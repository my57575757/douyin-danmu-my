//! IPC 命令：前端调用的全部入口
//!
//! 连接相关命令（connect_room / disconnect_room / get_connection_status）在 `connection.rs`。
//! 这里只做「读内存态 / 写内存态 + 广播事件 + 落盘」的薄封装，业务逻辑不落在此处。

use crate::config;
use crate::douyin::login_helper::{COOKIE_PREFIX, LOGIN_HELPER_ARG};
use crate::state::{prune_ticks, AppState, OverlayState};
use crate::tts;
use crate::window::{overlay_window, sync_sender_docked, DEFAULT_OVERLAY_H, DEFAULT_OVERLAY_W};
use serde_json::json;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub(crate) fn ping() -> String {
    "pong".into()
}

/// 显示 / 隐藏 Overlay（返回新可见状态）
#[tauri::command]
pub(crate) fn overlay_set_visible(
    app: AppHandle,
    state: State<'_, OverlayState>,
    visible: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    if visible {
        win.show().map_err(|e| format!("显示失败: {e}"))?;
    } else {
        win.hide().map_err(|e| format!("隐藏失败: {e}"))?;
    }
    *state.visible.lock().unwrap() = visible;
    Ok(visible)
}

/// 开关鼠标穿透（核心桌面功能：开启后鼠标事件穿透 Overlay 落到下层窗口）
#[tauri::command]
pub(crate) fn overlay_set_clickthrough(
    app: AppHandle,
    state: State<'_, OverlayState>,
    enabled: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_ignore_cursor_events(enabled)
        .map_err(|e| format!("设置穿透失败: {e}"))?;
    *state.clickthrough.lock().unwrap() = enabled;
    Ok(enabled)
}

/// 查询当前穿透状态
#[tauri::command]
pub(crate) fn overlay_get_clickthrough(state: State<'_, OverlayState>) -> bool {
    *state.clickthrough.lock().unwrap()
}

/// 查询 Overlay 是否可见（读内存态，理由见 `OverlayState::visible`）
#[tauri::command]
pub(crate) fn overlay_is_visible(state: State<'_, OverlayState>) -> bool {
    *state.visible.lock().unwrap()
}

/// 开关始终置顶
#[tauri::command]
pub(crate) fn overlay_set_always_on_top(
    app: AppHandle,
    state: State<'_, OverlayState>,
    enabled: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_always_on_top(enabled)
        .map_err(|e| format!("设置置顶失败: {e}"))?;
    *state.always_on_top.lock().unwrap() = enabled;
    Ok(enabled)
}

/// 仪表盘状态快照（主界面直播间页底部只读展示，前端按秒轮询）
#[tauri::command]
pub(crate) fn get_dashboard_status(app: AppHandle) -> serde_json::Value {
    let overlay = {
        let st = app.state::<OverlayState>();
        json!({
            "visible": *st.visible.lock().unwrap(),
            "clickthrough": *st.clickthrough.lock().unwrap(),
            "always_on_top": *st.always_on_top.lock().unwrap(),
        })
    };

    let tts = app.state::<tts::TtsState>().config();

    let now = Instant::now();
    let danmaku_rate = {
        let st = app.state::<AppState>();
        let mut ticks = st.danmaku_ticks.lock().unwrap();
        prune_ticks(&mut ticks, now);
        ticks.len()
    };

    json!({
        "overlay": overlay,
        "tts": { "enabled": tts.enabled, "filter": tts.filter },
        "danmaku_rate": danmaku_rate,
    })
}

/// 读取当前 Overlay 弹幕样式
#[tauri::command]
pub(crate) fn overlay_get_style(state: State<'_, OverlayState>) -> config::OverlayStyle {
    state.style.lock().unwrap().clone()
}

/// 更新 Overlay 弹幕样式：广播给 Overlay 窗口并持久化
#[tauri::command]
pub(crate) fn overlay_set_style(
    app: AppHandle,
    state: State<'_, OverlayState>,
    style: config::OverlayStyle,
) -> Result<(), String> {
    *state.style.lock().unwrap() = style.clone();
    let _ = app.emit("overlay-style", &style);
    config::save_overlay_style(&app, &style)?;
    // 弹幕字号变化影响发送框缩进与高度，同步重对齐
    sync_sender_docked(&app);
    Ok(())
}

/// 读取当前弹幕过滤配置
#[tauri::command]
pub(crate) fn danmaku_get_filter(state: State<'_, OverlayState>) -> config::DanmakuFilter {
    state.filter.lock().unwrap().clone()
}

/// 更新弹幕过滤配置：广播给 Overlay 窗口（实时生效）并持久化
#[tauri::command]
pub(crate) fn danmaku_set_filter(
    app: AppHandle,
    state: State<'_, OverlayState>,
    filter: config::DanmakuFilter,
) -> Result<(), String> {
    *state.filter.lock().unwrap() = filter.clone();
    let _ = app.emit("danmaku-filter", &filter);
    config::save_danmaku_filter(&app, &filter)
}

/// 读取当前礼物列表配置
#[tauri::command]
pub(crate) fn gift_get_config(state: State<'_, OverlayState>) -> config::GiftConfig {
    state.gift.lock().unwrap().clone()
}

/// 更新礼物列表配置：持久化（门槛与连击合并由 Rust 侧判定，广播供设置页回显）
#[tauri::command]
pub(crate) fn gift_set_config(
    app: AppHandle,
    state: State<'_, OverlayState>,
    gift: config::GiftConfig,
) -> Result<(), String> {
    *state.gift.lock().unwrap() = gift.clone();
    let _ = app.emit("gift-config", &gift);
    config::save_gift_config(&app, &gift)
}

/// 读取礼物朗读配置
#[tauri::command]
pub(crate) fn gift_tts_get_config(state: State<'_, tts::TtsState>) -> config::GiftTtsConfig {
    state.gift_config()
}

/// 更新礼物朗读配置：内存态实时生效并持久化（开关/门槛独立于弹幕朗读与礼物区）
#[tauri::command]
pub(crate) fn gift_tts_set_config(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    gift_tts: config::GiftTtsConfig,
) -> Result<(), String> {
    state.set_gift_config(gift_tts.clone());
    config::save_gift_tts_config(&app, &gift_tts)
}

/// 读取欢迎信息配置
#[tauri::command]
pub(crate) fn welcome_get_config(state: State<'_, OverlayState>) -> config::WelcomeConfig {
    state.welcome.lock().unwrap().clone()
}

/// 更新欢迎信息配置：持久化并广播
///
/// 欢迎信息在 Rust 侧完成去重与限速后才广播，前端只负责把收到的行渲染出来；
/// 关掉开关时 Rust 直接不广播，前端不需要再判一遍。
#[tauri::command]
pub(crate) fn welcome_set_config(
    app: AppHandle,
    state: State<'_, OverlayState>,
    welcome: config::WelcomeConfig,
) -> Result<(), String> {
    *state.welcome.lock().unwrap() = welcome.clone();
    let _ = app.emit("welcome-config", &welcome);
    config::save_welcome_config(&app, &welcome)
}

/// 读取进场欢迎语音播报配置
#[tauri::command]
pub(crate) fn welcome_tts_get_config(state: State<'_, tts::TtsState>) -> config::WelcomeTtsConfig {
    state.welcome_config()
}

/// 更新进场欢迎语音播报配置：内存态实时生效（关掉即丢弃排队中的欢迎）并持久化
#[tauri::command]
pub(crate) fn welcome_tts_set_config(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    welcome_tts: config::WelcomeTtsConfig,
) -> Result<(), String> {
    state.set_welcome_config(welcome_tts.clone());
    config::save_welcome_tts_config(&app, &welcome_tts)
}

/// 查询 Overlay 当前尺寸（逻辑像素，与 overlay_set_size 同一口径）
#[tauri::command]
pub(crate) fn overlay_get_size(app: AppHandle, state: State<'_, OverlayState>) -> Result<serde_json::Value, String> {
    // 窗口可能还没建好（主界面加载比 setup 建窗更快，首次安装尤其明显）：
    // 此时回退到建窗时已经落下的内存态尺寸，滑块才不会显示错
    let Some(win) = overlay_window(&app) else {
        let b = *state.bounds.lock().unwrap();
        return Ok(match b {
            Some(b) => json!({ "width": b.w.round(), "height": b.h.round() }),
            None => json!({ "width": DEFAULT_OVERLAY_W, "height": DEFAULT_OVERLAY_H }),
        });
    };
    let scale = win.scale_factor().map_err(|e| format!("读取缩放失败: {e}"))?;
    // inner_size 是物理像素，高 DPI 下得先换算，否则滑块显示的数会比实际小
    let size = win
        .inner_size()
        .map_err(|e| format!("读取尺寸失败: {e}"))?
        .to_logical::<f64>(scale);
    Ok(json!({ "width": size.width.round(), "height": size.height.round() }))
}

/// 调整 Overlay 宽高（设置页滑块调用；尺寸变化自动被窗口事件捕获落盘）
#[tauri::command]
pub(crate) fn overlay_set_size(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| format!("调整尺寸失败: {e}"))
}

/// 查询登录态
///
/// 抖音登录只是为了拿到推礼物的 Cookie（弹幕/进场/关注/点赞匿名就能收），
/// 故只回「登没登」；昵称要另找接口，不值当。
#[tauri::command]
pub(crate) fn get_login_info(state: State<'_, AppState>) -> serde_json::Value {
    let logged_in = state.auth.lock().unwrap().is_some();
    json!({ "loggedIn": logged_in })
}

/// 扫码登录：spawn 一个 helper 子进程（同一个 exe，带 `--login-helper`）去加载抖音登录页
///
/// 为什么不开在本进程：抖音首页会让 WebView2 自动播放视频流、把 GPU 进程顶到 1 GB 峰值，
/// 窗口销毁后还有约 84 MB 的 GPU 缓存残留（进程级，不随窗口回收）。子进程退出
/// 是唯一能确定全额归还内存的方式（见 `douyin/login_helper.rs`）。
///
/// 命令一直悬着直到子进程结束（成功 / 超时 / 窗口被关），调用方要自己显示进行中状态。
#[tauri::command]
pub(crate) async fn douyin_login_open(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("取程序路径失败：{e}"))?;
    // output() 会阻塞线程，丢进阻塞线程池等（helper 最长活 300 秒）
    let out =
        tauri::async_runtime::spawn_blocking(move || std::process::Command::new(exe).arg(LOGIN_HELPER_ARG).output())
            .await
            .map_err(|e| format!("等待登录进程失败：{e}"))?
            .map_err(|e| format!("启动登录进程失败：{e}"))?;

    if !out.status.success() {
        // 失败原因在 helper 的 stderr 上，转记进主程序日志（不把技术细节抛给用户）
        let reason = String::from_utf8_lossy(&out.stderr);
        log::warn!("[login] 登录进程未完成：{}", reason.trim());
        return Err("登录未完成（窗口被关闭或超时），请重试".into());
    }
    let cookie = helper_cookie(&out.stdout).ok_or("登录进程没有回传 Cookie，请重试")?;
    let auth = config::AuthInfo { cookies: cookie };
    config::save_auth(&app, &auth)?;
    *state.auth.lock().unwrap() = Some(auth);
    log::info!("[login] 抖音登录成功，Cookie 已加密落盘（礼物消息从下次连接开始可收）");
    Ok(())
}

/// 从 helper 的 stdout 里挑出 Cookie 那一行（helper 用前缀标记，避免混入其它输出）
fn helper_cookie(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .find_map(|line| line.strip_prefix(COOKIE_PREFIX))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// 退出登录（清本地登录态 + 清 WebView2 里的抖音登录）
///
/// 光清本地配置不够：WebView2 的 cookie 库里还登着，再点一次「登录」会秒过，
/// 用户会以为退出没生效。
/// - 登录页的 cookie 在 helper 的独立 WebView2 目录里（`login-webview`），整目录删掉
/// - 主程序的 WebView2 里可能还留着老版本的登录 cookie（那时登录页跑在主进程里），
///   再走一次 `clear_all_browsing_data` 兜底
#[tauri::command]
pub(crate) fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    *state.auth.lock().unwrap() = None;
    config::clear_auth(&app)?;
    if let Ok(dir) = crate::douyin::login::login_webview_dir(&app) {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => log::info!("[login] 已清除登录页 WebView2 目录"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("[login] 清除登录页 WebView2 目录失败：{e}"),
        }
    }
    if let Some(win) = app.get_webview_window(crate::douyin::signer::SIGN_WINDOW_LABEL) {
        let _ = win.clear_all_browsing_data();
    }
    Ok(())
}

/// 发送弹幕：首期不做（要另一套签名与登录 cookie，难度翻倍，见 `Task/task_plan.md` 已做的决策）
#[tauri::command]
pub(crate) fn send_danmaku(_msg: String) -> Result<(), String> {
    Err("抖音发送弹幕暂未支持".into())
}

/// 查询「已关注且正在开播」的主播（主界面手动刷新；只需登录态，与当前是否已连接房间无关）
#[tauri::command]
pub(crate) async fn list_following_live(
    state: State<'_, AppState>,
) -> Result<Vec<crate::douyin::live::FollowingLive>, String> {
    // 关注列表接口未登录返回 20003；锁作用域内克隆，避免持锁 await
    let cookies = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) if !a.cookies.trim().is_empty() => a.cookies.clone(),
            _ => return Err("请先扫码登录后查看关注在播".into()),
        }
    };
    crate::douyin::live::list_following_live(&crate::douyin::live::http_client(), &cookies).await
}

/// 查询最近连接的直播间（主界面面包屑，已按最近在前排序）
#[tauri::command]
pub(crate) fn get_recent_rooms(app: AppHandle) -> Vec<config::RecentRoom> {
    config::load_config(&app).recent_rooms
}

/// 读取 TTS 朗读配置
#[tauri::command]
pub(crate) fn tts_get_config(state: State<'_, tts::TtsState>) -> config::TtsConfig {
    state.config()
}

/// 更新 TTS 朗读配置：内存态实时生效（下一段朗读即生效）并持久化
#[tauri::command]
pub(crate) fn tts_set_config(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    tts: config::TtsConfig,
) -> Result<(), String> {
    state.set_config(tts.clone());
    config::save_tts_config(&app, &tts)
}

/// 试听当前音色/语速（不受总开关限制）
#[tauri::command]
pub(crate) fn tts_test_speak(app: AppHandle, text: Option<String>) {
    let text = text.unwrap_or_else(|| "试听效果，欢迎来到直播间".into());
    tts::speak_test(&app, &text);
}

/// 当前音色列表（内置中文音色兜底，拉取成功后为微软完整列表）
#[tauri::command]
pub(crate) fn tts_list_voices(state: State<'_, tts::TtsState>) -> Vec<serde_json::Value> {
    voice_options(&state.voices())
}

/// 从微软拉取完整音色列表（当前 322 个）并缓存到内存
#[tauri::command]
pub(crate) async fn tts_refresh_voices(app: AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let voices = tts::edge::fetch_voices().await?;
    let state = app.state::<tts::TtsState>();
    state.set_voices(voices.clone());
    Ok(voice_options(&voices))
}

fn voice_options(voices: &[(String, String)]) -> Vec<serde_json::Value> {
    voices
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect()
}

/// 打开日志目录（关于页「日志」按钮；排障时让用户能直接拿到日志文件）
///
/// 目录可能还不存在（本次运行还没写过日志、或用户手动清过），先建出来再开：
/// ShellExecuteW 对不存在的路径会返回 SE_ERR_* 而打不开。
#[tauri::command]
pub(crate) fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| format!("取日志目录失败: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
    crate::update::open_in_shell(&dir.to_string_lossy())
}
