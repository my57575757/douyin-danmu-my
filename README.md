<p align="center"><img src="imgs/app-icon.png" alt="douyin-danmu" width="128"></p>

<p align="center" style="font-size:36px; font-weight: bold;"> douyin-danmu</p>

<p align="center">抖音直播弹幕助手：桌面透明悬浮窗看弹幕 + Edge TTS 朗读，给用 OBS 推流的主播</p>

## 开发背景

- 抖音官方没有面向主播的桌面弹幕窗：手机看弹幕要抬手机，网页版弹幕又跟画面挤在一个窗口里。
- 现存的弹幕助手多基于臃肿的桌面框架，体积大、占用高。
- 因此本项目重写：给用 OBS 推流的主播一个不挡游戏画面、能挂在后台的看弹幕窗口。
- 骨架与 UI 来自同作者的 [bili-danmu](https://github.com/bddxg/bili-danmu)，协议层整个换成抖音。
- 感谢开源社区：弹幕协议相关接口的公开分析与实现，尤其 [DanmuFree](https://github.com/SoraYjy/DanmuFree) 与 [DouyinLiveWebFetcher](https://github.com/saermart/DouyinLiveWebFetcher)。

## 亮点

- **小**：安装包 3 MB（0.1.0 实测 3.08 MB）。不用额外装浏览器组件（Windows 自带 WebView2）、不用装 Python、不用下载语音模型，双击下一步就能用。
- **不跟游戏抢电脑**：直播时游戏已经满负荷，助手不该再压上一块石头；鼠标穿透一开，弹幕窗不拦鼠标，点击直接落到游戏上。
- **故意没做的功能**：没有用户头像、没有礼物图标，一行弹幕就是一行字；没有花哨动画；不打赏刷屏，同一个人送同一件礼物合并成一行显示总数量。
- **挂着就行**：断线自动重连，重连期间不弹报错、不刷屏；弹幕最多留 120 条，超出挤掉最早的，看一整天也不会卡。

## 能做什么

| 功能 | 说明 |
|---|---|
| 收弹幕 | 连上直播间就实时收，断线自动重连（1/2/4/…/64s 退避），掉线不用手动管 |
| 弹幕窗 | 桌面透明悬浮窗，可拖动、可缩放、可置顶、可鼠标穿透，背景不透明度可调，位置和大小自动记住；弹幕窗下方有穿透快捷开关 |
| 身份徽章 | 用户名前显示抖音等级（荣誉等级）；加入了粉丝团的观众再跟一枚灯牌徽章 |
| 只看想看的 | 等级门槛筛选 + 敏感词屏蔽（显示与朗读各一套，互不影响） |
| 礼物 | 显示在弹幕上方独立区域，可设金额门槛；连击只占一行显示总数量；也能插队念出来 |
| 欢迎信息 | 进房 / 关注 / 点赞提示行（默认关：这类消息比弹幕还多）；进房可单独开语音播报「欢迎某某进入直播间」，与弹幕朗读总开关互不影响 |
| 朗读 | 14 个中文音色（普通话 / 方言 / 粤语 / 台湾），语速音量可调；念之前会清洗 emoji 和链接 |
| 其他 | 最近房间一键直连、系统托盘常驻、运行状态一览、统一日志 |

**还没做**：发送弹幕（首期只读）、滚动弹幕、顶弹、用户屏蔽、Windows 系统 TTS、开机自启、全局快捷键、自动更新。

## 快速开始

> 只想用不想编译：到 [Releases](https://github.com/bd-dxg/douyin-danmu/releases/latest) 下载安装包（`douyin-danmu_x.y.z_x64-setup.exe`）双击安装即可。

1. 输入直播间号：浏览器打开直播间，地址 `live.douyin.com/` 后面那串数字就是（例如 `765790908625`），点「连接」
2. 弹幕实时显示在桌面透明悬浮窗；设置页可开鼠标穿透，让鼠标点击落到弹幕窗下方的窗口
3. 想收礼物消息：点「扫码登录」用抖音 App 扫码 —— **不登录也能收弹幕 / 进场 / 关注 / 点赞**，只有礼物需要登录态
4. 想听弹幕：到朗读页开启开关、选音色，点「试听」调好语速音量再挂后台

> 截图待补：真机跑通后再补弹幕窗与设置页截图（当前 README 只放图标）。

## 数据与隐私

- 配置存在 `%APPDATA%\com.douyindanmu.app\config.json`；登录 Cookie 加密保存（和本机、本机用户绑定），配置文件拷到别的电脑也读不出来，要重新扫码
- 不收集任何信息、不上传日志
- 唯一的对外请求：开启朗读后，弹幕文字会发往微软 Edge TTS 服务合成语音，关掉开关就停

## 构建

环境要求：Node.js ≥ 20、pnpm、Rust 工具链（MSVC）、WebView2 Runtime（Win10/11 自带）。

```powershell
pnpm install
pwsh -File scripts/copy-sign-js.ps1   # 准备签名脚本（见下方「签名 JS」）
pnpm tauri dev                        # 开发运行
pnpm tauri build                      # 打包（产物在 src-tauri/target/release/bundle/nsis/）
```

**签名 JS 不在本仓库里**：抖音的 X-Bogus 签名要跑 webmssdk 的 JS，那是字节跳动的反混淆产物、授权口径与本项目的 GPL-3.0 不一致，所以只提供准备脚本与命令（`probe/README.md` 的「从零准备签名 JS」）。缺了它应用能启动，但连不上直播间。

## 致谢与 License

本项目以 [GNU GPL v3](LICENSE) 开源。

| 来源 | 用途 | 授权 |
|---|---|---|
| [bili-danmu](https://github.com/bddxg/bili-danmu) | 骨架、UI 与整体设计（本项目由它派生） | GPL-3.0 |
| [DanmuFree](https://github.com/SoraYjy/DanmuFree) | 抖音协议蓝本：房间解析、签名素材、WS 帧结构 | MIT |
| [DouyinLiveWebFetcher](https://github.com/saermart/DouyinLiveWebFetcher) | protobuf 字段号对照表（**仅本地对照，不进仓库**） | AGPL-3.0 |
| webmssdk 签名 JS | X-Bogus 签名（字节跳动所有，**不进仓库**） | 第三方 |

> 技术实现（功能状态、协议细节、架构、构建与发布）见 [docs/技术说明.md](docs/技术说明.md)。
> 协议实测结论（字段号、坑、复跑探针）见 [probe/README.md](probe/README.md)。
