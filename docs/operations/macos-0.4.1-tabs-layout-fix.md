# DSH Client 0.4.1 macOS 标签栏遮挡修复预览

仅 macOS Apple Silicon（arm64），最低 macOS14；未签名、未公证。本版修复0.4.0在实际DSH页面打开后，远端原生WebView覆盖顶部标签栏的问题。节点、LAN/WG顺序、认证与SSH配置保持原有功能；没有构建其他平台或重启DSH Host。

## 根因与修复

用户`.13`截图显示主窗口标题栏下仅剩标签按钮上沿，远端DSH页面正常但遮住节点管理、节点切换和＋。依赖源码确认：Tauri2.12.5默认Visible标题栏启用full-size content view，WKWebView主页面会避让标题栏；子视图位置直接基于包含标题栏的父NSView。0.4.0在整窗原点下移56logicalpt，少算系统标题栏。

0.4.1通过AppKit公开API `NSWindow.contentLayoutRect` 取得未被标题栏遮挡的内容区域，经 `contentView.convertRect_fromView` 转为实际父视图坐标，并用与Wry一致的父frame高度换算到左上原点。在安全区顶部再预留56logicalpt给标签栏，远端高度取安全区高减56；初次创建、创建后复核及后续缩放/全屏事件共享此布局，不写死标题栏高度，不更改主壳frame或远端权限。

## 验证证据

- 回归先失败：titlebar-inset33pt场景，预期远端y89、旧代码y56。修复后通过；另覆盖无标题栏、其它inset、小窗口。
- 前端36项通过，typecheck和生产build通过。Rust27项通过、2项原有fixture/liveSSH显式忽略。
- 临时隔离原生诊断进程实际创建WKWebView，并在三个窗口尺寸查询native bounds：初始1180×780、960×640、1280×820；实际安全区顶32pt，scale2.0。三次actual均等于expected，远端y88（32+56），底部对齐安全区域；诊断返回Ok并退出。临时诊断代码未包含在源码提交或最终包内。
- 独立只读审查确认AppKit调用在主线程，当前主线程的with_webview立即派发，不会等待自身；初次和缩放共用坐标，无新增IPC/权限变化，无必须修复问题。
- `.13`用户实机验收：2026-11-17 19:48（Asia/Shanghai），用户在“0.4.1 实机检查”中选择“上述操作都正常”，确认打开节点后标签栏完整可点击、＋选择与节点切换正常，以及红色关闭窗口后可从Dock恢复。这是用户现场确认，区别于代理远程截图或独立审查；未单独确认全屏切换或新一轮模型流式请求。

## 包与安装

ZIP：`DSH-Client-0.4.1-macos-arm64-tabs-preview.zip`，2,217,303字节。
ZIP SHA-256：`2193b99869797a1745e935b0eb389ccf76f0a4ccf83d4408194b0bfdb8ff406c`。
APP二进制SHA-256：`5f025a5136a4b3b3f0edc6470302cb4a96d1aedd31a4d03c7823b2d9caae4326`。

2026-11-17 `.13` 主机 `zcj123vdeMacBook-Air.local` 已验证旧版0.4.0，正常退出客户端，再保留完整旧APP为 `~/Applications/DSH Client-0.4.0-before-0.4.1-20261117.app`，安装0.4.1到 `~/Applications/DSH Client.app` 并启动PID13162。版本0.4.1、bundle ID `com.zcj.dshclient`、arm64与安装二进制哈希均核对。DSH监听127.0.0.1:3080升级前后仍PID91076；无worker重启或自启注册，未修改客户端节点存储或Cookie。

若需回滚，先退出客户端，再恢复上述完整APP备份；0.3.0早期备份也仍保留。不要删除DSH Profile、凭据或客户端浏览分区。0.4.0视觉问题已有用户截图证据，不应再把它当成可验收版本。
