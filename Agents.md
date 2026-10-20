# douyin-danmu 开发指南

抖音直播弹幕桌面助手（Windows）：连接抖音直播间 → 实时接收弹幕 / 礼物 / 进场 → 桌面透明悬浮层显示 + 可选 Edge TTS 朗读，供 OBS 推流主播看/听弹幕。

- **协议实测结论与坑**：`probe/README.md`（含字段号权威表、复跑探针的命令）
- **功能状态与规划**：`docs/技术说明.md`（**以它为准**）；本文件写约定与注意事项
- 本项目的骨架与 UI 派生自 [bili-danmu](https://github.com/bddxg/bili-danmu)，协议层整个换成抖音；对照物在 `D:\code\githubpro\bili-danmu`

## 与 B 站版的根本差异（先看这几条，能省很多弯路）

| 维度 | 抖音 | 说明 |
|---|---|---|
| 房间号 | 用户输入**短号**（`live.douyin.com/` 后面那串），WS 只认 19 位 `room_id` | 短号 → room_id 由 `douyin::resolver` 换；抖音的号 `u32` 装不下，全链路是 `String` |
| 握手 | cookie 必须带 `ttwid` + URL 上带 `signature`(X-Bogus) | 签名要跑 webmssdk 的 JS（隐藏 WebView2）；登录 cookie 只能**合并**进去，不能替换掉 ttwid |
| 收帧 | 收到帧**必须回 ack** | 不回则服务端每 1–2 秒重推同一帧、cursor 不推进 |
| 心跳 | 10 秒 | 发一个 `PushFrame{f8=gzip(空)}` |
| 登录 | **只为礼物**：弹幕 / 进场 / 关注 / 点赞匿名就能收 | 礼物必须登录态（实测同房间：匿名 180 秒 0 条 / 登录 120 秒 8 条） |
| 打赏形态 | **只有礼物** | 没有醒目留言、没有舰队；免费礼物（钻石价 0）不算打赏 |
| 连击数量 | 服务端下发的是**累计值**（`combo_count`） | 本地**不要**再累加，否则连击数会变成平方 |
| 身份 | 只有「用户等级（荣誉等级）」+「粉丝团（灯牌）等级」 | 没有房管 / 舰队 / 荣耀等级 / 粉丝牌名 |
| 发送弹幕 | 不做（已探明） | 必须带 `a_bogus`，它只能由 webmssdk 生成 —— 唯一可行路径是隐藏窗口加载**整个直播间页**代发，内存开销大；目标用户（主播与一起玩的水友）有队伍麦，用不上。实测结论留档在 `probe/README.md` 的「发送弹幕」 |
| 观看会话 | 同一账号 + 同一直播间**只保留一个**（手机端 / 网页端 / 助手互相会顶） | 所以 `enter` 请求**只能带 ttwid、不能带登录态**，否则助手一连接就把网页端顶掉；登录态只交给 WS（礼物必需） |

## 技术栈

- 前端：Vue 3（`<script setup>`）+ Vite + TypeScript，包管理 pnpm
- 桌面壳：Tauri 2（Rust，edition 2021）
- Rust 关键依赖：tokio / reqwest（json、gzip）/ tokio-tungstenite（弹幕 WS + Edge TTS WS）/ native-tls（Edge TTS 自建 TCP 后包 TLS）/ futures-util / flate2（弹幕 gzip）/ md-5（签名素材）/ sha2（随机 hex / ConnectionId 生成）/ windows-sys（DPAPI、winmm 播放、ShellExecuteW）/ tauri-plugin-log / tauri-plugin-updater（自动更新：检查 / 下载 / minisign 验签 / NSIS 静默安装重启）
- 授权：GPL-3.0

## 常用命令

- 安装依赖：`pnpm install`
- 开发运行：`pnpm tauri dev`（Vite 热更 + Rust 调试一起跑；Rust 改动会自动重编译重启）
- 仅前端：`pnpm dev`；前端构建：`pnpm build`
- 打包：`pnpm tauri build`（产物在 `src-tauri/target/release/bundle/nsis/`）
- 纯 Rust：在 `src-tauri/` 下 `cargo check` / `cargo fmt` / `cargo test --lib`（`-- --ignored` 是 5 个联网测试：Edge TTS 合成、音色列表、时钟偏差恢复、突发失败率、更新清单）；格式化配置是 `src-tauri/rustfmt.toml`（max_width 120、4 空格），改完 Rust 先 `cargo fmt` 再提交，否则 diff 会混入格式噪音
- **签名 JS 不在仓库里**（第三方产物，见 `probe/README.md`）：新克隆的仓库先跑 `pwsh -File scripts/copy-sign-js.ps1`
- 探针（都在 `probe/node/`）：`ws-probe.cjs`（协议）、`walk-rooms.cjs`（沿 `similar_rooms` 找正在直播的房间，抓样本用）
  - 含中文的 `.ps1` 必须用 `pwsh` 跑（Windows PowerShell 5.1 会按 GBK 解码无 BOM 的 UTF-8 文件，中文被拆坏后连引号都会解析出错）

## 代码约定

### 目录

- `src/` 前端：`views/`（RoomView 房间、DanmakuView 弹幕、TtsView 朗读、StreamerView 主播分区、AboutView 关于）、`components/`（`FollowingLiveRooms.vue` 关注在播）、`composables/`、`overlay/`（悬浮层入口 + 单行渲染 + `MetaBadges.vue` 徽章列 + `row-style.ts` 描边）、`sender/`（弹幕窗下方的**穿透小控件**；发送弹幕未实现，无输入框）、`styles/settings.css`、`types/ipc.ts``
- `src-tauri/src/`：`lib.rs`（组装）、`state.rs`、`commands.rs`、`connection.rs`（会话循环 + 重连）、`window.rs`（窗口与穿透小控件吸附）、`gift.rs`（礼物列表：门槛 + 合并）、`welcome.rs`（欢迎信息：去重限速）、`tts/`（队列 `mod.rs`、文案 `text.rs`、礼物聚合 `gift.rs`、流水线 `worker.rs`、播放 `player.rs`、协议 `edge.rs`）、`config/`（读写 + 结构体 + DPAPI）、`update.rs`
- `src-tauri/src/douyin/`：`proto.rs`（手写 varint / 解帧 / ack / 心跳）、`parser.rs`（method → 事件）、`emote.rs`（表情弹幕解析）、`live.rs`（关注在播 + 我的直播间）、`resolver.rs`（短号 → room_id/ttwid/主播名）、`sign.rs`（13 参数拼串 + md5 + `Signer` trait）、`signer.rs`（隐藏 WebView2 签名器 + 命令回包路由）、`ws.rs`（单次会话 + 重连策略）、`login.rs`（登录页：建窗 / 读 Cookie / 等扫码）、`login_helper.rs`（登录子进程入口）、`event.rs`（事件模型）
- 多窗口：`index.html`（主窗）+ `overlay.html` + `sign.html`（隐藏签名页）+ 登录子进程自建的登录窗（label `login`）；`sender.html` 是弹幕窗下方的**穿透小控件**（`lib.rs` 的 `SENDER_WINDOW_ENABLED = true`）—— 发送弹幕不做（见上表），窗口里只放一个穿透开关；穿透开启后弹幕窗收不到鼠标事件，必须靠这个独立窗口才能关回来。Tauri 配置见 `src-tauri/tauri.conf.json` 与 `capabilities/default.json`（新增窗口要加进 `windows` 列表）
- **主窗不在 `tauri.conf.json` 里定义**（`app.windows` 是空的，改在 `lib.rs` 的 setup 里建）：那个数组是整个 exe 共用的，helper 模式下也会被自动建出来，而 helper 的 WebView2 必须用独立 user data folder，自动建的窗口设不了 `data_directory`

### 硬约定

- 注释与 commit 一律简体中文
- **布局契约（改一处必须同步三处）**：`MetaBadges.vue` 的 `.level-slot`（徽章列宽 5.6em + 右间距 0.7em）→ `OverlayApp.vue` 的 `--level-indent`（6.3em）→ `window.rs` 的 `LEVEL_INDENT_EM`（6.3）。穿透小控件（sender 窗口）的左缩进用的是同一个数，不同步就会吸附错位。列宽是**量出来的**（最长「等级 + 灯牌」徽章组合 5.28em），要改就重新量
- `MetaBadges.vue`、`row-style.ts` 是弹幕行与礼物行**故意共用**的（两区正文列要对齐），别复制一份改单侧
- 弹幕窗性能敏感（120 条上限、透明层重绘）：入场动画只动 `transform` / `opacity`，**不要**给行加 `will-change`，也不要逐帧改 height / margin
- 日志一律 `log::info!/warn!/error!`，**禁止 `println!` / `eprintln!`**（release 是 `windows_subsystem = "windows"`，stderr 直接丢）；日志里不得出现完整 Cookie
- IPC 双向类型：Rust command ↔ `src/types/ipc.ts` 必须同步；**Tauri 命令名 = Rust 函数名**（改函数名记得同步前端的 `invoke('...')`）
- 登录 Cookie 在 `config.rs` 的唯一写盘边界 `save_config_unlocked` 统一 DPAPI 加密（`dpapi:` 前缀 + hex）；新增敏感字段必须走同类加密，内存态保持明文
- `config.rs` 读写持有全局互斥（`lock_cfg()` + `*_unlocked` 模式）：新增保存函数沿用这个模式，别在持锁时调用加锁的公共入口
- `src/styles/settings.css` 是全局样式（无 scoped），类名是所有页面的共用契约，加规则前先确认不撞车
- 删除功能时把孤儿代码一起删掉（本项目 `cargo check` 保持 **0 警告**，dead-code 清零是验收项）

## 协议层注意事项（每条都是踩过的坑）

1. **WS 握手必须带 `ttwid`**：用登录 cookie 整串替换 → `HTTP 200 + named cookie not present`。`resolver::merge_cookie` 负责合并
2. **签名只认 13 个参数的原值**：顺序在 `sign::SIGN_PARAM_ORDER`，不可重排；URL 上其它参数（`internal_ext`）必须转义但不能参与签名。单测里有一条「与 node 实跑值一致」的断言（md5 = `5af9eb2bbced94bfd670f58fd47aced2`），改了参数它会先炸
3. **签名失败不重试**（`WsError::retryable() == false`）：签名失败是环境问题，退避重试只会把同一次失败再跑一遍。握手被拒 / 读写失败才退避重连（`reconnect_delay`：1/2/4/…/64s，10 次上限）
4. **`eval` 拿不到 JS 的返回值**：签名走「Rust `eval` 触发 → 页面 `invoke('douyin_sign_reply', {id, bogus})` → oneshot 按 id 路由」。页面没就绪时回 `NOT_READY`，Rust 内部重试 10×200ms（别改成直接报错）
5. 隐藏窗口（`visible(false)`）里 WebView2 **照跑 JS**（本机已验证：启动日志 `[sign] WebView2 签名自检通过：X-Bogus=…`）
6. 解不开的帧**只丢帧不重连**（抖音会夹带格式异常的帧，重连代价是多跑一轮签名）
7. `proto::inflate` 只看偏移 0 的 gzip 魔数（探针是扫全串）。真机若出现「帧解不出 Response」而单测全绿，先怀疑这里
8. **登录窗口不能窄**：抖音页面是响应式的，窄到 ~600px 以下顶部栏折叠、「登录」入口直接不渲染（真机踩过：520 宽的窗口里根本找不到登录按钮）。现在是 1180×820 / 最小 1000×680
9. 退出登录要清两处：登录子进程的 `login-webview` 目录（整目录删）+ 主程序 WebView2 的 `clear_all_browsing_data()`（老版本把登录页跑在主进程里，可能留了 cookie），否则「退出」后一点登录就秒过
10. **登录页跑在独立子进程**（`douyin-danmu.exe --login-helper`）：抖音首页自动播放视频流会把 WebView2 的 GPU 进程顶到 1 GB 上下（`login.rs` 里已用 `--autoplay-policy` 关掉），窗口销毁后还有约 84 MB 残留。分流必须在 `lib.rs` 注册单实例插件**之前**，否则子进程会被主实例当成「第二个实例」顶掉
11. **helper 的 WebView2 必须用独立 user data folder**（`login-webview`）：WebView2 不允许两个环境（两个进程）指向同一个目录。给窗口设 `data_directory` 是 `WebviewWindowBuilder` 的能力，这也是 `tauri.conf.json` 的 `app.windows` 必须清空的原因
12. **`additional_browser_args` 会整串替换 wry 的默认参数**：默认是 `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`，自己传参时必须把它带上
13. **关注在播走 `POST https://www.douyin.com/webcast/feed/follow_top/`，不需要签名**：query 只要 `device_platform=webapp&aid=6383&channel=channel_pc_web`，body 只要 `source_key=web_homepage_follow_top&follow_session_id=<游标>`（页面真实请求里的 `a_bogus` / `msToken` / `uifid` / `x-secsdk-csrf-token` 全可省）。少了 body 里的 `source_key` 就回 `10011 Request params error`，**别往签名上排查**。返回里 `data[].owner.nickname` 是主播名、`web_rid` 是**直播间短号**（不是抖音号）、`data.id_str` 是真实 room_id，翻页看 `extra.follow_session_id` / `extra.has_more`
14. **`enter` 请求绝不能带登录 cookie**（`resolver.rs` 的 `enter_cookie_for`，有单测卡着）：抖音对「同一账号 + 同一直播间」只保留一个观看会话 —— 手机端与网页端进同一直播间也会互相顶（实测）。带登录态调 `enter` 等于让账号「从助手进入直播间」，结果是把网页端的观看会话顶掉（真机现象：助手一连上网页就弹「账号已在其他地方进入直播间」）；匿名调它我们什么也不少（`room_id` / 主播名 / 开播状态匿名一样给）。登录态只交给 WS，礼物只在登录态下推
15. **「我的直播间」靠 `profile/self` 拿抖音号**（`live.rs` 的 `fetch_self_user`）：`GET https://www.douyin.com/aweme/v1/web/user/profile/self/`（只带登录 cookie，**不需要签名**）→ `user.unique_id` 就是抖音号（自定义过的抖音号可能是字母，那就用永远是数字的 `short_id`），而 `enter` **认抖音号当 `web_rid`**（实测 `data.user.id_str` 与自己的 `uid` 一致），于是自己那条直接用抖音号走既有连接流程。三条别搞错：`enter` **不认 uid、也不认 room_id**（都回 `4001038`）；抖音**没有** B 站那种「按 uid 查开播状态」的接口，而 `profile/self` / `profile/other` 的 `room_id_str` **未开播就归 0**（实测：已下播但房间还在的号也是 0），所以这条**不看开播状态**，房间不存在时靠 `resolver` 的正常报错收场。注：抖音号可以是字母（实测 `88866kzi`，`enter` 与 `live.douyin.com/{抖音号}` 都认），所以 `connect_room` 与输入框的校验是**数字 / 字母都收**——别再改回纯数字，否则自定义抖音号的主播看不到「我的直播间」（带 `_` 的抖音号不收）

## 身份徽章（等级 / 灯牌）的字段口径

| 展示 | 字段 | 备注 |
|---|---|---|
| 用户等级（荣誉等级） | `User.f23 = PayGrade` → `f6`(varint) | 读不到就从图标 URL 抠 `new_user_grade_level_v1_N.png`；`User.f21` 徽章列表里也挂着同一张图 |
| 灯牌等级 | `User.f24 = FansClub{f1 = FansClubData{f2 = level}}` | 没加团的观众 f24 是十字节空壳；**灯牌没有名字**（`clubName` 实测为空） |

- **不要用 `User.f6`**：它绝大多数用户根本不带，带的人值是 1，**不是等级**。真机曾因此显示错（显示的其实是粉丝团等级 / 或凭空一个 LV1）
- 徽章列宽必须够：`.level-slot` 是 `justify-content: flex-end` + `overflow: hidden`，内容超宽从**左边**裁（真机出过「LV20 显示成 V20」）

## 朗读（Edge TTS）

- 协议集中在 `tts/edge.rs`（音色表 `edge/voices.rs`、SSML `edge/ssml.rs`、DRM 令牌 `edge/drm.rs`）：改协议用 `scripts/edge-tts-probe.ps1` 验证（`-SelfTest` 可离线自检令牌算法）
- `Sec-MS-GEC` 令牌是 **v2**（`tts/edge/drm.rs`，`Sec-MS-GEC-Version: 2-…`）：微软 2026-10 起强制，v1（纯 SHA256 + `1-` 前缀）在部分地区已握手上报 403，现象是本地能朗读、别人不能。v2 = `<16 hex CS64>-<64 hex SHA256>`，两套哈希的常数都被刻意改过，**不能**换成 `sha2` 标准实现；唯一可信的回归判据是官方向量单测 `cargo test --lib drm`。另外 ticks 必须走浮点路径，别「优化」成整数运算——官方实测某些时间戳会因此错开、令牌失配
- 握手被 403 拒绝时 `connect` 会按响应头 `Date` 校准本机时钟偏差再重试一次（`drm.rs` 的 `CLOCK_SKEW_MS`，官方同款）：偏差超过 5 分钟窗口时令牌怎么算都错、表现同样是「朗读全挂、弹幕正常」，本机时钟不准的用户靠这条自愈（联网回归测试 `edge_recovers_from_clock_skew_live`）
- 播放走 winmm MCI + 固定临时文件；依赖队列串行（同一时刻只有一个 MCI 句柄）；`max_len` 只约束弹幕正文
- 停播有两条**互不相同**的路径，别合并：①`epoch` 递增 = 作废还没开播的在途音频；②`player::play_mp3` 的 `stop_now` 闭包 = 每 100ms 查总开关。`force = true`（试听 / 礼物朗读）两道都免疫
- **Edge TTS 不接受连接复用**（同一连接发第二轮必被 RST），`synthesize` 每条新建连接，别改成连接池
- 建连走自研 `dial_tcp`（自己解析 DNS、IPv4 优先）：别换回 `tokio_tungstenite::connect_async`，国内 IPv6 直连微软偶发被 RST，现象是弹幕正常而朗读整段全挂
- 合成失败必须走熔断退避（连续 2 次 → 清积压 + 1/2/4…封顶 60s），排查用 `cargo test --lib -- --ignored edge_burst_live --nocapture`
- 单条合成有两层超时（`worker.rs` 的 `SYNTH_TIMEOUT` 与 `edge.rs` 的 `RECV_TIMEOUT`），别只留一个
- 礼物朗读自己按「用户 + 礼物」聚合一遍（不复用 `gift.rs` 的分组表），静默满一个连击窗口才念一次，插队用 `push_front`；队列丢弃只丢**弹幕**（`force` 项保留）
- 欢迎信息里只有**进房**可朗读（文案「欢迎某某进入直播间」，`WelcomeTtsConfig` 独立开关、默认关）；关注 / 点赞不朗读（抖音没有舰队档位，念出来没有信息量）。进房事件先过 `welcome.rs` 的去重限速（默认 60 秒同人去重 + 30 秒最多 1 条，两道各有开关）才入朗读队列，所以热房间不会刷屏；播报项是 force 项（不受弹幕总开关与打断作废影响），但合成前会再查一次它自己的开关，关掉后排队中的欢迎整条不播
- 表情弹幕有两副面孔，**必须两类都处理**：①`WebcastEmojiChatMessage` 是**独立消息类型**，正文就是表情名，弹幕窗渲染成「[表情名]」；②网页端还会把表情写成 `[名字]` 文本发在**普通弹幕**里（实测「[发呆][发呆]」，报文没有表情字段）—— `douyin::emote::strip_bracket_emotes` 按正文形态认它。朗读默认两类都不念（`TtsConfig.read_emote`，设置页在「朗读筛选」页签）：①整条不念、②剥掉 `[名字]` 片段只念剩下的文字（代价：手打的方括号也被剥）。只判 `d.emote.is_some()` 会漏掉 ②，现象是「开关关了还在念表情」。表情名取不到时 `parser` 打一条 warn、带出 payload 字段号，**别改成静默丢弃**（抖音改字段号时全靠这条排查）
- 提及弹幕（正文开头的 `@名字`）念成「发送者对被提及者说 正文」：抖音**没有**独立的回复消息类型（探针抓到的 method 里只有 `WebcastChatMessage`），只能按正文形态认（拆在 `douyin/mention.rs`、组装在 `tts/text.rs`）。**不看** `read_username`：这类弹幕不念两边的名字就丢了「对谁说」；只 @ 了人、后面没说话时整条跳过；正文中间的 `@` 不算提及（手打的不改句式）

## 礼物 / 欢迎信息

- `gift.rs` 是礼物金额门槛与连击合并的**唯一判定处**（前端只按 id 覆盖与条数上限）；数量与金额**取服务端累计值**，不累加
- 欢迎信息在 `welcome.rs`：同人同类 60 秒去重 + **所有形态共用一个大池**每 30 秒最多 1 条；两道闸各有开关（`WelcomeConfig::dedupe_enabled` / `rate_limit_enabled`，默认都开），关掉的闸不写自己的状态表（避免切开关后陈旧记录误挡）；去重限速**与显示 / 语音开关无关地先跑**（放行后显示与语音播报是两个独立消费者，各看各的开关：`OverlayState.welcome` / `TtsState.welcome_config`）
- 礼物朗读门槛与礼物区门槛**各自独立**（`GiftTtsConfig` 可更低甚至只朗读不显示）

## 版本号与发布

- 禁止在 `main` 上直接开发：功能 / 修复 / 重构先切分支（`feature/` / `fix/` / `refactor/`），完成后开 PR、squash 合并回 `main`；**只有版本号提升、README / 文档这类收尾可直接提交 `main`**
- 版本号同步 3 处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`（`Cargo.lock` 由构建自动更新）
- 流程：改版本 → commit（`🚀 应用版本号升至 X.Y.Z（…）`）→ 推 `main` → `pnpm tauri build`（**必须带签名密钥**）→ 生成 `latest.json` → `gh release create`

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\douyin-danmu.key"   # 私钥不进仓库；值是文件路径即可
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""   # 空密码也要显式设：只设上面那个，CLI 会弹密码提示并永久等输入
pnpm tauri build
pwsh scripts/gen-latest-json.ps1 -Version 0.3.0   # 生成 latest.json（顺带校验版本号三处已同步）
```

- 自动更新靠 `latest.json` 当端点内容，**每次发布都要把它和 exe / exe.sig 一起上传**；漏传 = 用户那边「检查更新失败」
- 打包时必须**同时**设 `TAURI_SIGNING_PRIVATE_KEY`（私钥路径）与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（空密码就写空串）：只设前者时 CLI 会停在 `Decrypting updater signing key, expect a prompt for password` 等输入，表现为「构建莫名其妙卡了很久」
- `@tauri-apps/cli` 必须 >= 2.12.3：只有它会在 updater 签名里写 `version` 字段，而 `requireSignedVersion` 开着时客户端靠这个字段挡降级——旧 CLI 签出的签名（没有 version）会被**直接拒绝**，现象是「下载完装不上」
- 更新走加速镜像（`gh-proxy.org` 等三个 + GitHub 直连兜底），端点写在 `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`：**改端点要发版**，所以要增删镜像得连同版本号一起走流程
- `tauri-plugin-updater` 的 Rust 依赖锁在 `~2.12`、npm 包锁在 `2.12.0`：上游 2.13+ 要求 tauri ^2.12，而 tauri 还停在 2.11.5（升 tauri 会连带换掉 wry / tao，悬浮窗最脆，单独一个 PR 再动）。两边版本号要一致，tauri CLI 在 build 时会校验
- 安装包路径：`src-tauri/target/release/bundle/nsis/douyin-danmu_<版本>_x64-setup.exe`
- 打包前确认应用没在运行（NSIS 写 exe 会失败）；`gh release create` 的 tag 只建在远端，本地要 `git fetch --tags`
- 历史一律 squash 合并（main 上每个 PR 一个带 `(#N)` 的 commit）：`gh pr merge --squash` **别传 `--subject`**，否则 GitHub 不会自动追加 `(#N)`

## 注意事项

- 禁止提交：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/schemas/`、`dev.log`、根目录截图
- **第三方代码/产物不进仓库**（授权口径与本项目 GPL-3.0 不一致，`.gitignore` 已配）：
  - `probe/sign/sign.js`、`probe/browser/sign_browser.js`、`public/douyin-sign.js` —— webmssdk 反混淆产物（字节跳动所有）
  - `probe/reference/` —— 来自 saermart/DouyinLiveWebFetcher（**AGPL-3.0**）
  - `probe/node/*.txt` —— 探针跑出来的日志与 **Cookie 导出**（含凭据，尤其别外传）
- 配置与日志目录：`%APPDATA%\com.douyindanmu.app\config.json`、`%LOCALAPPDATA%\com.douyindanmu.app\logs\douyin-danmu.log`
- 房间不在直播时（响应里 `status != 2`）WS 仍能连上但基本没消息：`connection.rs` 会打一条 warn，别把「连上了没弹幕」当 bug 查
- 抖音的「用户等级」筛选开着会滤掉大部分弹幕（没消费过的观众没有等级）——这是设计使然，不是 bug
- 自动更新：检查 / 下载 / 验签 / 安装全在 `tauri-plugin-updater`，**前端不要自己拼版本比较或调 relaunch**——Windows 上插件把安装包交给 NSIS 后直接 `process::exit(0)`，由安装器（passive + `/R`）装完重启；清单端点与 minisign 公钥在 `tauri.conf.json` 的 `plugins.updater`，权限单独放在 `capabilities/updater.json` 且只给主窗口
- 防降级：`plugins.updater.requireSignedVersion` 开着（前提：CLI >= 2.12.3，签名里才会带 `version`）。篡改清单把「新版本号 + 旧发布包」拼在一起会被客户端拒装；`gen-latest-json.ps1` 会校验签名绑定的版本与清单一致
- 更新提示位置：侧边栏「关于软件」的红点 + 关于页胶囊按钮（一个按钮承载检查 / 下载 x% / 安装中）。**不往弹幕窗提示**——弹幕窗会被 OBS 采集上镜
