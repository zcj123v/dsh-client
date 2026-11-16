# DSH Client

DSH Client 是一个纯客户端壳，连接已经运行的 DSH Web Host。它不安装、不启动、不升级 DSH，也不承载 Agent 执行过程。

## 当前范围

- macOS、Windows、Linux 桌面：每个连接一个独立 WebView 窗口。
- Android：连接列表与当前 WebView；SSH-forward 需要设备已有 VPN/direct HTTPS，首版不伪造系统 SSH。
- direct：加载已有 HTTPS/域名入口。
- ssh-forward：复用系统 OpenSSH 和 `~/.ssh/config` 的 Host 别名，将目标 `127.0.0.1:<remotePort>` 转发到本机随机 loopback 端口。
- 首次打开时用户在 DSH 根页面粘贴启动 token；token 和 Basic Auth 不写入客户端配置。WebView 按连接持久化 Cookie，删除连接才清理分区。

## 开发

```bash
pnpm install --frozen-lockfile
pnpm dev                 # 浏览器壳
pnpm tauri dev           # Tauri 桌面壳
pnpm test
pnpm typecheck
pnpm build
source "$HOME/.cargo/env"
cargo check --manifest-path src-tauri/Cargo.toml
```

前置：Node.js 22+、pnpm、Rust stable；macOS 构建需要 Xcode Command Line Tools。Android 还需要 Android Studio/SDK/NDK，执行 `pnpm tauri android init` 后再构建。

## 构建

```bash
pnpm tauri build --target aarch64-apple-darwin
pnpm tauri build --target x86_64-pc-windows-msvc
pnpm tauri build --target x86_64-unknown-linux-gnu
```

本轮测试版不签名、不公证、不注册自启。发布前另行配置代码签名、公证、Windows Authenticode 和 Linux 仓库产物。

## 安全边界

远端页面不获得 Tauri IPC；外部链接交给系统浏览器。SSH 参数始终以参数数组传给 `ssh`，不拼接 shell。客户端配置只保存连接元数据和分区 ID，不保存 Provider token、SSH 私钥、DSH_HOME、Session 数据库或 Fleet secret。

详见：

- [架构](docs/architecture.md)
- [安全](docs/security.md)
- [本机 macOS](docs/operations/local-macos.md)
- [MacBook Air .13](docs/operations/macbook-air-13.md)
