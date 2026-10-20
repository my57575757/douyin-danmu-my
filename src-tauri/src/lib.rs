//! douyin-danmu 核心入口
//!
//! 连接流程：connect_room(短号) → resolver 换真实 room_id + ttwid → 签名（隐藏 WebView2）
//!   → WS 握手 → 10s 心跳 + 回 ack → 解出抖音事件 → 分发给弹幕窗 / 朗读 / 礼物 / 欢迎
//! 登录：抖音只在**登录态**下推礼物消息（弹幕/进场/关注/点赞匿名就可收），
//! 登录跑在独立的 helper 子进程里（`douyin/login_helper.rs`），Cookie 持久化时经 DPAPI 加密。
//!
//! 模块分工：状态见 `state.rs`，IPC 命令见 `commands.rs`，连接与重连见 `connection.rs`，
//! 窗口辅助见 `window.rs`；本文件只做组装（状态托管、日志插件、窗口创建、托盘、命令注册）。

mod commands;
mod config;
mod connection;
mod douyin;
mod gift;
mod state;
mod tts;
mod update;
mod welcome;
mod window;

use state::{AppState, OverlayState};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use window::{capture_overlay_bounds, flush_overlay_bounds, sync_sender_docked, DEFAULT_OVERLAY_H, DEFAULT_OVERLAY_W};

/// 是否创建底部吸附小窗（label `sender`）
///
/// 发送弹幕未实现（要另一套签名 `a_bogus` + 登录 cookie，见 `docs/技术说明.md`），
/// 所以窗口里**不放输入框**，只放一个「穿透」开关按钮：穿透开启后弹幕窗不再接收
/// 鼠标事件，放在弹幕窗里的按钮会点不到，必须有个不随穿透失效的独立窗口来关它。
/// 实现发送后把 `SenderApp.vue` 换回输入框即可（窗口定义 / `send_danmaku` 命令都保留着）。
const SENDER_WINDOW_ENABLED: bool = true;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 登录 helper：同一个 exe 的第二种身份（见 douyin/login_helper.rs）。
    // 必须在注册单实例插件**之前**分流，否则本进程会被已在运行的主实例
    // 当成「第二个实例」顶掉，登录页永远打不开。
    if std::env::args().any(|a| a == douyin::login_helper::LOGIN_HELPER_ARG) {
        douyin::login_helper::run_login_helper();
        return;
    }

    let app = tauri::Builder::default()
        // 单实例：必须最先注册——第二个实例在插件初始化阶段就退出，
        // 不会走到 setup 去建托盘/Overlay/连接；同时唤出已有实例的主窗口
        // （点托盘关闭后主窗是隐藏态，需先 show 再聚焦）
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        // 统一日志：level / 落盘目录 / 轮转策略都在这里定，业务代码只调 log::* 宏
        //
        // 默认 max_file_size 只有 40KB 且 KeepOne（超限即丢旧文件），对「出事翻日志」
        // 毫无用处，故放大到 5MB 并保留最近 7 份（轮转出的旧文件会带上日期后缀）。
        // 时区必须显式切本地时间：默认 UTC，国内用户对着日志还原现场会差 8 小时。
        // release 是 windows_subsystem = "windows"，没有可用的 stdout，只挂 LogDir；
        // dev 额外挂 Stdout，保留 `pnpm tauri dev` 的终端输出。
        .plugin({
            let mut targets = vec![tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                file_name: None,
            })];
            if cfg!(debug_assertions) {
                targets.push(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout));
            }
            tauri_plugin_log::Builder::new()
                .targets(targets)
                .level(log::LevelFilter::Info)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(7))
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build()
        })
        // 自动更新：端点（国内加速优先、GitHub 直连兜底）、公钥、Windows 安装模式
        // 都在 tauri.conf.json 的 plugins.updater 里，这里不另传参数
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            status: Mutex::new(state::RoomStatus::Disconnected),
            cancel: Mutex::new(None),
            auth: Mutex::new(None),
            danmaku_ticks: Mutex::new(VecDeque::new()),
        })
        .manage(OverlayState::default())
        .manage(tts::TtsState::new(config::TtsConfig::default()))
        .manage(gift::GiftState::default())
        .manage(welcome::WelcomeState::default())
        .setup(|app| {
            // 主窗口在这里手动建，不再由 tauri.conf.json 的 app.windows 自动创建：
            // 那个数组是整个 exe 共用的，helper 模式下也会被自动建出来，而 helper 的
            // WebView2 必须用独立 user data folder（见 douyin/login_helper.rs），
            // 只有手动建窗才能给单个窗口设 data_directory。
            WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("douyin-danmu")
                .inner_size(880.0, 650.0)
                .min_inner_size(520.0, 420.0)
                .resizable(true)
                .center()
                .build()?;

            // 加载持久化配置：登录态 → AppState.auth；样式/位置 → OverlayState
            let cfg = config::load_config(app.handle());
            if let Some(auth) = &cfg.auth {
                let st = app.state::<AppState>();
                *st.auth.lock().unwrap() = Some(auth.clone());
                log::info!("[auth] 已恢复抖音登录态（礼物消息需要它）");
            } else {
                log::info!("[auth] 未登录：能收弹幕/进场/关注/点赞，收不到礼物");
            }
            {
                let ov = app.state::<OverlayState>();
                *ov.style.lock().unwrap() = cfg.overlay_style.clone();
                *ov.filter.lock().unwrap() = cfg.danmaku_filter.clone();
                *ov.gift.lock().unwrap() = cfg.gift.clone();
                *ov.welcome.lock().unwrap() = cfg.welcome.clone();
            }
            // TTS：恢复朗读配置并启动串行朗读 worker（与弹幕显示完全解耦）
            app.state::<tts::TtsState>().set_config(cfg.tts.clone());
            app.state::<tts::TtsState>().set_gift_config(cfg.gift_tts.clone());
            app.state::<tts::TtsState>().set_welcome_config(cfg.welcome_tts.clone());
            if cfg.gift_tts.enabled {
                log::info!("[tts] 已恢复礼物朗读开关，门槛={} 元", cfg.gift_tts.min_amount_yuan);
            }
            if cfg.welcome_tts.enabled {
                log::info!("[tts] 已恢复进场欢迎语音播报开关");
            }
            if cfg.tts.enabled {
                log::info!("[tts] 已恢复朗读开关，音色={}", cfg.tts.voice);
            }
            tts::spawn_worker(app.handle().clone());

            // 抖音签名：隐藏 WebView2 窗口（跑 webmssdk 的 getSign）
            // 启动就建，避免第一次点「连接」时还要等页面加载；托管签名器供命令回包路由。
            // 扫码登录不再复用这个窗口（跑在独立 helper 进程里，见 douyin/login_helper.rs）
            app.manage(douyin::signer::WebviewSigner::new(app.handle().clone()));
            if let Err(e) = douyin::signer::create_window(app.handle()) {
                log::error!("[sign] 签名窗口创建失败: {e}");
            }
            // 自检：webmssdk 能不能在真 WebView2 里跑起来是阶段 5 的待确认项，
            // 启动打一条日志就能验证，不必等真连房间（失败只 warn，不影响启动）
            {
                let signer = app.state::<douyin::signer::WebviewSigner>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    match signer.self_check().await {
                        Ok(bogus) => log::info!("[sign] WebView2 签名自检通过：X-Bogus={bogus}"),
                        Err(e) => log::warn!("[sign] WebView2 签名自检失败：{e}"),
                    }
                });
            }

            // Overlay：透明 / 无边框 / 置顶 / 可调整 / 跳过任务栏，恢复上次位置
            let mut win_builder = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
                .title("douyin-danmu overlay")
                .inner_size(DEFAULT_OVERLAY_W, DEFAULT_OVERLAY_H)
                .position(80.0, 80.0)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .resizable(true)
                .min_inner_size(120.0, 40.0)
                .skip_taskbar(true)
                .shadow(false);
            if let Some(b) = cfg.overlay_bounds {
                win_builder = win_builder.position(b.x, b.y).inner_size(b.w, b.h);
            }
            if let Ok(win) = win_builder.build() {
                // 建窗即显示：先把内存态写成与之一致，前端此刻还没读到也能拿到真实值
                *app.state::<OverlayState>().visible.lock().unwrap() = true;
                capture_overlay_bounds(app.handle());
                let app2 = app.handle().clone();
                win.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                        capture_overlay_bounds(&app2);
                        // 发送框始终吸附：弹幕窗移动/缩放时跟随
                        sync_sender_docked(&app2);
                    }
                });
            } else {
                *app.state::<OverlayState>().visible.lock().unwrap() = false;
                log::error!("[overlay] 创建失败");
            }
            // 周期落盘窗口位置（拖动/缩放中去重保存）
            let app3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    flush_overlay_bounds(&app3);
                }
            });

            // Sender：底部吸附小窗（穿透开关）始终吸附在弹幕窗下方（无边框 / 置顶 / 跳过任务栏）。
            // 发送弹幕未实现，故窗口里只放「穿透」按钮（见 SENDER_WINDOW_ENABLED 的说明）。
            if SENDER_WINDOW_ENABLED {
                let sender_builder = WebviewWindowBuilder::new(app, "sender", WebviewUrl::App("sender.html".into()))
                    .title("douyin-danmu 穿透")
                    .inner_size(96.0, 32.0)
                    .decorations(false)
                    .transparent(true)
                    .always_on_top(true)
                    .resizable(false)
                    .skip_taskbar(true)
                    .shadow(false);
                if sender_builder.build().is_err() {
                    log::error!("[sender] 创建失败");
                }
                // 创建后立即对齐到弹幕窗下方（覆盖默认位置/尺寸）
                sync_sender_docked(app.handle());
            }

            // 主窗口 × → 最小化到托盘（不退出；由托盘菜单唤出/退出）
            if let Some(main_win) = app.get_webview_window("main") {
                let app2 = app.handle().clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = app2.get_webview_window("main").map(|w| w.hide());
                    }
                });
            }

            // 系统托盘：左键单击显示设置窗口
            let show_item = MenuItem::with_id(app, "show", "显示设置", true, None::<&str>)
                .map_err(|e| log::error!("[tray] 创建菜单项失败: {e}"))
                .ok();
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
                .map_err(|e| log::error!("[tray] 创建菜单项失败: {e}"))
                .ok();
            let mut items: Vec<&dyn tauri::menu::IsMenuItem<_>> = Vec::new();
            if let Some(i) = &show_item {
                items.push(i);
            }
            if let Some(i) = &quit_item {
                items.push(i);
            }
            if let Ok(menu) = Menu::with_items(app, &items) {
                let tray = TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().expect("default window icon").clone())
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            // 先销毁窗口再退出：destroy 绕过主窗 ×→隐藏 拦截，
                            // 让 WebView2 在进程退出前释放 HWND，避免 1412 unregister 竞态
                            let wins: Vec<_> = app.webview_windows().into_values().collect();
                            for w in wins {
                                let _ = w.destroy();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .build(app)
                    .map_err(|e| log::error!("[tray] 创建托盘失败: {e}"))
                    .ok();
                let _ = tray; // 托盘由 tauri 管理，持有即保活
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            connection::connect_room,
            connection::disconnect_room,
            connection::get_connection_status,
            commands::get_login_info,
            commands::logout,
            commands::douyin_login_open,
            commands::send_danmaku,
            commands::list_following_live,
            commands::overlay_set_visible,
            commands::overlay_set_clickthrough,
            commands::overlay_get_clickthrough,
            commands::overlay_is_visible,
            commands::overlay_set_always_on_top,
            commands::get_dashboard_status,
            commands::overlay_get_style,
            commands::overlay_set_style,
            commands::danmaku_get_filter,
            commands::danmaku_set_filter,
            commands::gift_get_config,
            commands::gift_set_config,
            commands::gift_tts_get_config,
            commands::gift_tts_set_config,
            commands::welcome_get_config,
            commands::welcome_set_config,
            commands::welcome_tts_get_config,
            commands::welcome_tts_set_config,
            commands::tts_get_config,
            commands::tts_set_config,
            commands::tts_test_speak,
            commands::tts_list_voices,
            commands::tts_refresh_voices,
            commands::get_recent_rooms,
            commands::overlay_get_size,
            commands::overlay_set_size,
            update::open_url,
            commands::open_log_dir,
            douyin::signer::douyin_sign_reply,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // 退出前落盘 Overlay 窗口位置
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            flush_overlay_bounds(handle);
        }
    });
}
