# macOS Native Download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 macOS DSH Client 节点内容页中提供可靠的「下载 / 另存为」，将 `.6` 的文件保存到 `.13` 本机，支持 257 MB APK。

**Architecture:** 在实际节点 WebView 注入最小 `window.dshDesktop.fileActions` 桥接；Rust 独立校验页面身份和同源 URL，复用当前节点分区 Cookie，按 Range 有界读取并原子保存。不修改远端 DSH Host。

**Tech Stack:** Rust、Tauri 2、Wry/WKWebView、reqwest blocking、tauri-plugin-dialog、TypeScript/Vitest。

**Spec:** `docs/superpowers/specs/2026-11-23-macos-native-download-design.md`

## Global Constraints

- 本期仅 macOS；Windows/Linux/Android 行为不变。
- 只接受已登记节点内容页发起的同源 HTTP(S) 下载，拒绝 URL 凭据与任意本地路径参数。
- 不将 token/Cookie 写入日志、文件名、返回值或测试；不重启目标 DSH、不注册自启。
- 下载按有界块读写；默认不覆盖已有文件；失败/取消清理临时文件。
- 保留所有既有未提交配置导入导出改动；不得 reset/clean 或覆盖用户工作。
- 当前工作树为 `/Users/zhangchengjie/Workspace/dsh-client-priority`，分支 `preview/macos-node-priority`；在既有隔离工作树内实施。
- 只有用户已批准的 `.13` 备份/替换客户端属于交付范围；不自动发布 GitHub、不改其他平台。

## Review Focus

- WKWebView 自定义协议与同源：实际节点 URL 是 `http://tauri.localhost/` 的本地壳还是远端 loopback，分别验证桥接 origin 与 cookie 读取。
- 页面在异步读取/保存期间关闭或换标签：不得让旧请求写入新的选择结果。
- 257 MB 文件及超过 32 MiB：不得以 RPC 完整读取替代 HTTP Range。
- 服务端忽略 Range、错误 Content-Range 或短读：不得将重复/截断内容标为保存成功。
- 既有文件、符号链接与同名临时文件：不得意外覆盖或写到目录外。

---

### Task 1: 原生下载边界与流式保存核心

**Files:**
- Create: `src-tauri/src/downloads.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

**Interfaces:**
- Consumes: `Connection.urls` 当前节点 origin、`partition_id(id)`；现有 `reqwest` blocking 与 `tauri-plugin-dialog`。
- Produces: macOS `downloads::save(app, webview, request) -> Result<SaveOutcome, String>`；`SaveOutcome::{Saved(PathBuf),Cancelled}`；`DownloadRequest` 仅含 `url/name/media_type/mode`。

- [ ] **Step 1: 写失败测试**

在 `downloads.rs` 单元测试中覆盖：
- `validate_source` 接受节点登记的 HTTPS 与 loopback HTTP，拒绝跨 origin、URL 用户名/密码、非 HTTP(S) 和未知节点。
- `safe_file_name` 清理 `/`、`\\`、`.`/`..`、控制字符和空名称，保留 `.apk`。
- `next_available` 对 `a.apk` 生成 `a.apk` 或 `a (1).apk`，不覆盖既有文件。
- 本地测试 HTTP 服务支持 `Range: bytes=0-1048575`，以固定块读取 >32 MiB 的合成数据；验证完整 SHA-256、偏移和 eof。
- 服务端忽略 Range 时，只允许 offset 0 的 `200`；后续 `200`、错误 `206`、短读均失败并删除临时文件。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: 因 `downloads` 模块不存在而 FAIL。

- [ ] **Step 3: 实现独立 Rust 下载核心**

定义请求与保存结果；由调用方传入已校验的 origin 与节点 Cookie，而不从任意网页接收 Cookie。以 `reqwest::blocking` 发起 Range，逐块写入同目录随机临时文件；首个响应接受 `206` 或精确匹配的 `200`，后续必须 `206` 且 Content-Range 与请求范围一致。写完后同步并原子重命名；所有错误路径清理临时文件。Cookie 仅放内存且只发送给同一 origin。

- [ ] **Step 4: 验证下载核心**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: 所有 downloads 测试 PASS；合成大文件哈希一致，失败无临时文件。

- [ ] **Step 5: 提交**

仅提交本任务新增模块与必要依赖：`git add src-tauri/src/downloads.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock`，使用 `git diff --cached` 核对不含配置导入既有改动后提交。

### Task 2: 节点 WebView 桥接、对话框与生命周期

**Files:**
- Modify: `src-tauri/src/downloads.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/tabs.rs`
- Create: `src/file-actions.ts`
- Test: `tests/file-actions.test.ts`

**Interfaces:**
- Consumes: Task 1 `downloads::save`；macOS `tabs.rs` 的 `dsh-<UUID>` 节点 WebView；非 macOS `open_connection` 的节点窗口；现有 `Connections.urls`。
- Produces: 节点 WebView 中 `window.dshDesktop.fileActions.save({url,name?,mediaType?,mode}) -> Promise<{status:'saved',path:string}|{status:'cancelled'}>`；仅 macOS 可用，其他平台不注册。

- [ ] **Step 1: 写桥接失败测试**

测试 `installFileActionsBridge` 的纯校验/状态行为：合法 download/saveAs 调用进入原生 command，参数仅 URL/name/mediaType/mode；非法 mode、空 URL、相对 URL 与非 HTTP(S) 被拒绝；不得包含 token、Cookie、path。Rust 测试验证 `require_node_content` 只接受登记 `dsh-<UUID>` 标签或窗口，拒绝 `main`、`clear-<UUID>` 与未登记 label；请求 generation/owner 不匹配时拒绝旧请求落盘。

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run tests/file-actions.test.ts && cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: 桥接与节点身份接口尚未实现而 FAIL。

- [ ] **Step 3: 注入最小桥接并实现原生命令**

通过 `on_initialization_script` 在每个实际节点内容 WebView 注入 `window.dshDesktop.fileActions`。只实现 `save`；`readClipboardImage` 返回明确 unsupported，避免新增跨源读取。JS 调用 Tauri command，并防止多实例覆盖桥接。

Rust command 通过调用者 WebView 校验节点 label、登记 origin、URL 同源与生命周期；异步任务中保存弱引用/代际，关闭或替换节点后取消并清理。macOS `saveAs` 使用系统对话框选择路径，下载模式从系统 Downloads 目录取可用名称；返回原始字节已保存的绝对路径。使用 `tokio::task::spawn_blocking`/Tauri 等价机制，避免阻塞 UI。

在 macOS `tabs.rs` 节点 builder 与生命周期中接线；不修改普通节点 builder 的其他平台行为。

- [ ] **Step 4: 验证桥接与全量回归**

Run: `pnpm test && pnpm typecheck && cargo test --manifest-path src-tauri/Cargo.toml --lib && pnpm build`
Expected: 全部通过；Android 不改变无桥接行为。

- [ ] **Step 5: 提交**

仅提交本任务文件，先核对 staged diff 不包含既有配置导入导出改动。

### Task 3: macOS 构建与 `.13` 实际验收

**Files:**
- Modify: `docs/operations/macos-native-download.md`（可由本任务创建）

**Interfaces:**
- Consumes: Task 1–2 的下载实现与现有 macOS 打包命令。

- [ ] **Step 1: 构建并验证包**

运行完整测试，使用 `pnpm tauri build --bundles app`，再以 `ditto` 生成 arm64 ZIP。记录版本、架构、APP/可执行文件 SHA-256；不声称签名/公证。

- [ ] **Step 2: 备份与安装 `.13`**

只读确认主机为 `zcj123vdeMacBook-Air.local`、arm64；若客户端在运行，先正常退出并检查 SSH 转发清理。完整保留旧 `~/Applications/DSH Client.app` 为带日期的备份，再替换安装新 APP。不注册自启、不重启 DSH worker，记录前后 DSH 监听 PID。

- [ ] **Step 3: 实测 DSH 下载**

在 `.13` 连接 `.6` 的现有 DSH 会话，使用专用小测试文件验证「另存为」及取消；再用 257 MB APK 验证「下载」到 `.13` 本机 Downloads，核对文件大小与 SHA-256 `56a3ec6e1cadb61b58432c8e1df4068acda27700056dc4b86d7804a90ce9b5e1`。验证临时文件清理与既有文件不覆盖。不得用公开网页、编译成功或 HTTP 401 替代业务验收。

- [ ] **Step 4: 记录与交付**

文档记录实际版本/哈希、备份路径、下载位置、已验证项和仍未验证的平台；提交仅本任务文档。
