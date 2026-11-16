# Local macOS

## Prerequisites

```bash
node --version
pnpm --version
rustc --version
xcode-select -p
```

Rust 缺失时安装官方 stable toolchain：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

## Verify

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

## Run

```bash
pnpm tauri dev
```

添加 direct 连接时使用本机 DSH Web URL，例如 `http://127.0.0.1:3080/`。首次打开根页面粘贴当前进程打印的 token。添加 SSH-forward 连接时填写 `~/.ssh/config` 中的 Host 别名和远端 DSH 端口，客户端会启动本机 loopback 随机端口转发。

## Build test artifact

```bash
pnpm tauri build --target aarch64-apple-darwin
shasum -a 256 src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg
```

未签名测试包可能触发 Gatekeeper；只对本地个人测试使用，必要时在 Finder 中右键打开或移除文件 quarantine。不要把未签名包当作发布产物。
