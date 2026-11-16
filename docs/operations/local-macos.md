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
