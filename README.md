# DSH Client

连接已运行 DSH Web Host 的纯客户端。客户端不安装、启动或托管 DSH，不打包 Node、执行体、Provider 配置或私钥。

## 当前交付状态

| 平台 | 状态 |
| --- | --- |
| macOS 14+ arm64 | 已构建未签名 APP；本机已验证 token 登录、完整界面；`.13` 用户目录安装测试 |
| Windows / Linux x64 | GitHub 原生 runner 已通过测试、编译并生成 NSIS / DEB / AppImage；实际用户桌面连接仍待验收 |
| Android arm64 | 原生内容 Activity + AndroidX 独立 Profile 已实现；debug APK 已编译，API36 模拟器隔离/重建/权限与原生调用链通过；个人真机及真实 DSH 业务待验 |

桌面 v1 为连接列表 + 每连接独立窗口。单窗口真标签不属于本轮验收。

## 使用

1. 新建连接，填写名称。
2. HTTPS/Web：Android 只使用已有 HTTPS 入口；桌面可输入已有 HTTPS 域名或本机 `127.0.0.1:3080`。保存地址不能含 token、密码、查询参数、片段或反代子路径。非本机 HTTP 被拒绝。
3. SSH 转发（桌面）：选择 `~/.ssh/config`、`~/.ssh/dsh-fleet-config` 或不读取配置，填写 Host 别名、可选用户和目标 DSH 端口（默认 3080）。转发目标固定为远端 `127.0.0.1`。系统 OpenSSH 必须已能通过密钥无交互登录，主机指纹须在终端预先核对。
4. 首次连接，在左侧该连接的“一次性 token”框粘贴 **token 字符串**，然后“打开 / 重试”。不粘贴完整 URL。输入会清空，连接配置不保存 token；系统 WebView 保存认证 Cookie。
5. 之后打开时 token 留空即可。认证失效时重新填 token。编辑后保存会关闭旧连接窗口，下一次打开采用新配置。
6. 删除连接会关闭窗口、停止 SSH，并请求系统异步清除该分区浏览数据；系统 API 不提供同步完成保证，删除后不复用旧分区 ID。

SSH 本地端口按连接 ID 稳定分配在 40000–59999，以保留同一 origin 的 LocalStorage；端口占用时拒绝打开，避免加载已有的陌生服务。它不是任意 shell 命令入口，不接受任意 SSH 参数/配置路径。服务专用 key 不一定允许转发；`.13` 已验证“不读取配置 + 10.250.0.13 + zcj123v”的普通用户密钥路径。

## 开发与验证

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm typecheck
pnpm build
source "$HOME/.cargo/env"
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri dev
```

前置：Node.js 22+、pnpm、Rust stable；macOS 需要 Xcode Command Line Tools。仅安装 Tauri CLI 不代表已完成 Android/Windows/Linux 工具链和构建。

## macOS 测试产物

```bash
pnpm tauri build --target aarch64-apple-darwin --bundles app --no-sign
ditto -c -k --sequesterRsrc --keepParent 'src-tauri/target/aarch64-apple-darwin/release/bundle/macos/DSH Client.app' DSH-Client-arm64.zip
shasum -a 256 DSH-Client-arm64.zip
```

测试包未签名/公证，不注册自启。当前系统 Finder AppleScript 打包 DMG 曾超时，APP + ZIP 是本轮可验证分发路径。目标系统实际构建命令和产物须在该平台验证后再发布。

- [架构](docs/architecture.md)
- [安全](docs/security.md)
- [本机 macOS](docs/operations/local-macos.md)
- [MacBook Air .13](docs/operations/macbook-air-13.md)

## 各平台构建

- 桌面 CI：`.github/workflows/desktop.yml`，三平台原生 runner，依次安装依赖、运行测试、构建安装包并上传 Actions artifact（14天）。Windows 需要 WebView2 Runtime、系统 OpenSSH；Linux 需要 WebKitGTK 4.1，发行版依赖见生成 DEB。
- Android CI：`.github/workflows/android.yml`，构建 arm64 debug 主 APK 和设备测试 APK，不自动安装到个人设备；[Android 原生容器与验收](docs/operations/android.md)。
- 2026-11-16 首轮 [CI 43808066571](https://github.com/zcj123v/dsh-client/actions/runs/43808066571)：Windows/Linux 两个 job 成功并已收集产物；macOS 旧 runner 镜像被 GitHub 拒绝，已改用 macos-latest，本机 arm64 构建和实测仍有效。
