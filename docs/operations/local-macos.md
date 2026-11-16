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
