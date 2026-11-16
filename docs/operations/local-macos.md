# 本机 macOS 开发与验收

需要 Node 22+、pnpm、Rust stable、Xcode Command Line Tools；macOS 最低 14。

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
pnpm build
source "$HOME/.cargo/env"
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri dev
pnpm tauri build --target aarch64-apple-darwin --bundles app --no-sign
```

测试连接地址 http://127.0.0.1:3080/。先保存不带凭据的根 URL，再在连接条目的密码输入框粘贴一次性 token 字符串并打开；首次登录应显示完整 DeepSeek Harness 工作区与会话界面。仅看到 401 只证明认证栅栏存在，不能认定登录或 WebSocket 已通。

验收至少包含一次最小对话、无 token 重新打开（Cookie）、两个连接的隔离、SSH 连通、窗口关闭和退出回收。登录后当前地址不得残留 token。不要把浏览器预览页当作原生客户端测试。

macOS DMG 打包器曾因 Finder AppleEvent 超时失败；本轮使用 --bundles app + ditto ZIP，不依赖 Finder 安装。不要从未签名测试包宣称已完成签名/公证发布。


## 0.2.1 SSH 恢复与 WKWebView 分区验收

2026-11-16 本机 development App（本地壳 localhost1420）打开两个连接到同一临时HTTP origin127.0.0.1:4176。A/B首次 Cookie/LocalStorage均空；关闭A并从连接列表重开，恢复相同假标记及Cookie；两个真实原生窗口可同时存在。远端虽有Tauri内部对象，调用close_connection被权限/来源边界拒绝。未使用真实token或改DSH监听；探针仅临时本地服务。

SSH 回归使用真正HTTP子进程测试首次ready后kill→同port恢复、三次预算、退避取消、端口抢占拒绝，以及旧窗口teardown不杀替换task；并以获授权普通用户`.13` SSH路径实测kill转发→重建→kill/wait和端口重绑。该测试只核实传输，不把预登录401当业务成功。

恢复会重新导航到根页，未发送的页面编辑内容可能丢失；只恢复系统Cookie，不保留/重放一次性token。三次失败后用户可手动再试。系统OpenSSH的保活最长检测延迟由15秒×3控制；不是连接断开立即报告。


0.2.1本机未签名arm64 APP/ZIP构建成功并实际启动显示tauri://localhost连接列表；ZIP SHA256 `b90ccce0eb5fa922c955696e1bce6bcb232f8105c24a1726e698642b91f25436`。初轮macOSCI两个Python HTTP fixture等待超时，替换为Rust测试程序自身的独立HTTP子进程后最终四端CI全部通过（桌面43829650070、Android43829655132）；未屏蔽测试。生产实现自784e29a至a2c5601未变，仅测试fixture变更。审查者复核任务实例、窗口weak-slot、exactview与UIgeneration后无阻断项。


## 0.2.1 发布应用真实附件验收（2026-11-16 18:0x）

本轮使用已经构建的macOS arm64 0.2.1发布APP（PID8341，非浏览器替身），连接本机127.0.0.1:3080。实际完成本地一次性密码框→token/Cookie→完整DSH工作区、会话和原生文件选择对话框→选择仅含假数据的111B文本→网页待发送附件→发送测试消息。会话“DSH Client 附件链路验收”显示附件111B、已完成和 `DSH_CLIENT_ATTACHMENT_OK`。

不能仅凭模型回复认定文件到达：本机DSH的 `attachments/v1/file-objects/e7/e7c0898b89eefcc21c81a4c7ec640fc1a953561a873a1ebe7b70ae53855bf48c` 与 `attachments/v1/files/e7/<同SHA>/DSH-Client-Attachment-Smoke-20261116.txt` 都逐字节等于原始111B文件，SHA256 `e7c0898b89eefcc21c81a4c7ec640fc1a953561a873a1ebe7b70ae53855bf48c`。最初误查图片objects路径未找到，改查普通file-objects后实际核对通过；无伪造上传HTTP请求或替换文件选择回调。

随后关闭连接窗口6074，从主列表空token重开6186，完整认证页面和刚才附件会话直接恢复，根URL不带query。主列表有“窗口已关闭”状态，重开后正常。此次只做关闭/重开，不能扩大为系统重启持久化或网络物理断开；前轮已有0.1.1进程重启Cookie与0.2.1SSH恢复证据。

GUI自动化遇到坐标/索引混用、密码框焦点与原生对话框快捷键不生效，改为新截图坐标和原生picker当前Documents目录中唯一合成测试文件；确认选定文件名与111B后才发送。SystemEvents脚本未返回，已停止；Documents合成副本已核路径/字节后删除，服务端假附件和测试会话保留作证据。token只经本机脚本直接pbcopy到密码框，未打印或入仓，提交后剪贴板清空。命令行token探针必须保留303重定向的Cookie，普通urlopen无CookieJar会错误地落到401。

`.13`再次只读核对为zcj123vdeMacBook-Air.local/arm64，安装仍0.1.1，二进制SHA256 `b08209ed82a499de332e849d20ade9c87450412cff50ae88cca975991af5fa69`，worker仍10813。未更新用户现场测试版本、未重启Host。macOS本轮只验证普通文本附件上传和返回，不宣称Android/WinLinux真实DSH上传或通用下载通过。


## 原始测试版交付结论（2026-11-16）

独立只读交付复核household-newapi/gpt-6.1-sol核对原始人类批准和上述证据后，判定独立仓库/DSH子模块/中文规范与操作文档/本机macOS跑通/向`.13`安装测试版这些原始交付均已完成。桌面v1连接列表+独立窗口是明确批准的范围，单窗口真标签延期；Android为HTTPS-only单视图。四端有原生构建，macOS有真实DSH认证/会话/附件/恢复，Android有模拟器真实容器与9+1+1测试，不能把构建扩大为四端个人设备业务已通过。

`.13`0.1.1已安装并提供哈希/回滚和测试步骤，处于**已交付供用户测试，用户业务验收待反馈**；不宣称用户验收通过，不自动替换用户现场版本。可选证书pinning、物理同步擦除、其他个人设备业务、长时真实断网和统一细分状态等继续列入后续B19，不自动抬升为本次测试版交付门槛，也不宣称已实现。原始交付目标可结束，后续依据明确需求或现场反馈继续。
