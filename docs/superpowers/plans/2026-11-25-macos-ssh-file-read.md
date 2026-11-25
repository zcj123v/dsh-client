# macOS 经 SSH 读取远端文件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `.13` 的 DSH Client 里，把 `.6` 上的文件经 SSH `cat` 流式保存到本机，支持 257 MB APK，不修改任何 DSH 官方模块。

**Architecture:** 客户端在节点 WebView 里包装 `window.__ModuleLoader__.load`，只对官方 `ui-file-actions` 包的 `factory` 生效，把它的 `save` 换成"直接传原始 URL 给 Rust、不预读 Blob"的版本。Rust 从 URL 解析出远端路径，用该连接已有的 SSH 配置执行 `cat -- <path>`，stdout 流式写入本地临时文件后原子重命名。

**Tech Stack:** Rust、Tauri 2、std::process::Command、TypeScript（无运行时代码，Rust 以字符串注入）。

**Spec:** `docs/superpowers/specs/2026-11-25-macos-ssh-file-read-design.md`

## Global Constraints

- 不修改任何 DSH 官方文件；`.6` 的官方包保持未修改状态。
- 本软件为用户自用内部工具，不在客户端内部假设网页或客户端自身被攻破。不做路径白名单、`realpath` 往返、命令名白名单。
- SSH 调用用 `Command::new("ssh")` 加参数数组，路径经 `--` 之后的单独参数传入。这是为了避免路径含空格或特殊字符时出错，不是安全措施。
- 复用该连接已有的 SSH 配置（`sshConfig`、`sshProfile`、`sshUser`），不新增密钥。
- 直接模式（非 SSH 转发）的连接不支持下载，返回明确错误。
- 保留工作树中所有既有未提交改动（Android 验收、配置导入导出等），不 `reset`/`clean`。
- 当前工作树：`/Users/zhangchengjie/Workspace/dsh-client-priority`，分支 `preview/macos-node-priority`。
- `.13` 上保留现有预览版，不回滚。

## Review Focus

- 路径含空格、中文、`--` 前缀、换行时，SSH 参数是否仍把整个路径当作一个参数传给 `cat`。
- `ssh` 进程被用户在保存对话框取消后关闭标签页时，是否回收进程并删除临时文件。
- 远端 `cat` 返回非零（文件不存在、权限不足）时，错误信息是否能区分于本地写入失败，且不把远端 stderr 原样显示。
- 257 MB 文件流式写入时内存占用是否保持在缓冲区大小量级，不随文件增长。
- 挂钩在官方包 `id` 不匹配时是否完全不介入，页面其他功能不受影响。

---

### Task 1: 从 URL 解析远端路径，构造 SSH 读取参数

**Files:**
- Modify: `src-tauri/src/downloads.rs`
- Test: `src-tauri/src/downloads.rs`（`mod tests`）

**Interfaces:**
- Consumes: 无。
- Produces:
  - `fn remote_path_from_url(url: &Url) -> Result<String, String>`：从 `api/file?path=<p>` 取出 `p`，空值或非绝对路径返回错误。
  - `fn ssh_read_args(profile: &str, user: Option<&str>, config: Option<&str>, path: &str) -> Result<Vec<String>, String>`：返回 `-F <config>`（config 为 `Some` 时）、`-o BatchMode=yes`、`-o StrictHostKeyChecking=yes`、`[-l user]`、`profile`、`--`、`cat --`、`path`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn remote_path_from_url_extracts_absolute_path() {
    let url = Url::parse("http://127.0.0.1:40286/api/file?path=/Users/x/Workspace/a%20b.apk").unwrap();
    assert_eq!(remote_path_from_url(&url).unwrap(), "/Users/x/Workspace/a b.apk");
    for bad in ["http://127.0.0.1:40286/api/file", "http://127.0.0.1:40286/api/file?path=", "http://127.0.0.1:40286/api/file?path=relative/x.apk"] {
        assert!(remote_path_from_url(&Url::parse(bad).unwrap()).is_err(), "{bad}");
    }
}

#[test]
fn ssh_read_args_passes_path_as_single_argument_after_double_dash() {
    let args = ssh_read_args("dsh-mac", Some("u"), Some("/tmp/ssh_config"), "/p/a b/--x.apk").unwrap();
    let dash = args.iter().position(|a| a == "--").unwrap();
    assert_eq!(args[dash + 1], "cat");
    assert_eq!(args[dash + 2], "--");
    assert_eq!(args[dash + 3], "/p/a b/--x.apk");
    assert_eq!(args.iter().filter(|a| *a == "--").count(), 2);
    assert!(args.contains(&"-F".to_string()) && args.contains(&"/tmp/ssh_config".to_string()));
    assert!(args.contains(&"-l".to_string()) && args.contains(&"u".to_string()));
    assert!(args.contains(&"BatchMode=yes".to_string()));
    let no_config = ssh_read_args("dsh-mac", None, None, "/x").unwrap();
    assert!(!no_config.contains(&"-F".to_string()) && !no_config.contains(&"-l".to_string()));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: FAIL，`remote_path_from_url` 和 `ssh_read_args` 未定义。

- [ ] **Step 3: 实现两个函数**

`remote_path_from_url`：取 `query_pairs()` 中 key 为 `path` 的第一个值，空或非 `/` 开头返回 `Err("远端路径无效")`。

`ssh_read_args`：按 Produces 里的顺序拼接。`profile`/`user` 沿用 `policy::ssh_args` 里的 `safe` 校验规则；不合法返回 `Err("SSH Host 或用户无效")`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: 两个新测试 PASS，既有测试不受影响。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/downloads.rs
git commit -m "feat(macos): parse remote path and build ssh read args"
```

只暂存 `downloads.rs`；`lib.rs`、`Cargo.toml`、`Cargo.lock` 含其他未提交改动，不在本任务暂存范围。

---

### Task 2: 经 SSH 流式读取远端文件到本地

**Files:**
- Modify: `src-tauri/src/downloads.rs`

**Interfaces:**
- Consumes: Task 1 的 `remote_path_from_url`、`ssh_read_args`。
- Produces: `fn stream_ssh_to_file(args: &[String], output: &mut fs::File) -> Result<(), String>`：spawn `ssh`，把 stdout 复制到 `output`，等待退出；非零退出返回 `Err`，错误信息不含远端 stderr 内容。

- [ ] **Step 1: 写失败测试**

测试用本地 `sh -c` 模拟远端：把 `ssh_read_args` 的结果替换为 `["-c", "cat -- <path>"]` 交给 `stream_ssh_to_file` 的内部 helper。为此把 helper 签名定为 `fn stream_command_to_file(program: &str, args: &[String], output: &mut fs::File) -> Result<(), String>`，`stream_ssh_to_file` 是它传 `"ssh"` 的薄包装。

```rust
#[test]
fn stream_command_to_file_copies_stdout_and_reports_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("源 文件.bin");
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&src, &payload).unwrap();
    let mut out = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    let path = src.to_str().unwrap();
    stream_command_to_file("sh", &["-c".into(), format!("cat -- '{}'", path)], out.as_file_mut()).unwrap();
    assert_eq!(fs::read(out.path()).unwrap(), payload);

    let mut out2 = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    let err = stream_command_to_file("sh", &["-c".into(), "cat -- /nonexistent/file; exit 1".into()], out2.as_file_mut()).unwrap_err();
    assert!(!err.contains("No such file"), "remote stderr leaked: {err}");
    assert_eq!(fs::read(out2.path()).unwrap().len(), 0);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: FAIL，`stream_command_to_file` 未定义。

- [ ] **Step 3: 实现 `stream_command_to_file` 和 `stream_ssh_to_file`**

`Command::new(program).args(args).stdin(null).stdout(piped).stderr(piped)`，用 `std::io::copy` 把 stdout 写到 `output`（64 KiB 缓冲，`io::copy` 默认），另起线程读完 stderr 并丢弃，`wait()` 后非零返回 `Err("远端读取失败")`。`stream_ssh_to_file` 只做 `stream_command_to_file("ssh", args, output)`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: 新测试 PASS，既有测试不受影响。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/downloads.rs
git commit -m "feat(macos): stream remote file over ssh into local file"
```

---

### Task 3: 接入 `save_download`，替换 HTTP 路径

**Files:**
- Modify: `src-tauri/src/downloads.rs`
- Modify: `src-tauri/src/tabs.rs`

**Interfaces:**
- Consumes: Task 1、Task 2。
- Produces:
  - `pub fn resolve_ssh_config(app: &AppHandle, ssh_config: Option<&str>) -> Result<Option<PathBuf>, String>`：把 `"default"`/`None` → `None`，`"none"` → `/dev/null`，`"fleet"` → `~/.ssh/dsh-fleet-config`。放在 `downloads.rs`，`tabs.rs` 原有的同逻辑改为调用它。
  - `Connections` 新增字段 `ssh: Mutex<HashMap<String, SshTarget>>`，`SshTarget { profile: String, user: Option<String>, config: Option<PathBuf> }`。`open_connection` 创建 SSH 隧道时写入。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn resolve_ssh_config_maps_each_option() {
    let app = tauri::test::mock_app();
    assert_eq!(resolve_ssh_config(app.handle(), None).unwrap(), None);
    assert_eq!(resolve_ssh_config(app.handle(), Some("default")).unwrap(), None);
    assert_eq!(resolve_ssh_config(app.handle(), Some("none")).unwrap(), Some(PathBuf::from("/dev/null")));
    let fleet = resolve_ssh_config(app.handle(), Some("fleet")).unwrap().unwrap();
    assert!(fleet.ends_with(".ssh/dsh-fleet-config"));
    assert!(resolve_ssh_config(app.handle(), Some("other")).is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml downloads -- --nocapture`
Expected: FAIL，`resolve_ssh_config` 未定义。

- [ ] **Step 3: 实现 `resolve_ssh_config` 并让 `tabs.rs` 复用**

把 `tabs.rs` 里 `match record.ssh_config.as_deref()` 那段改成调用 `resolve_ssh_config`，返回的 `Option<PathBuf>` 再转成 `Option<&str>` 传给 `ssh_args`。行为不变。

- [ ] **Step 4: 在 `Connections` 里记录 SSH 目标**

`lib.rs` 的 `Connections` 加 `ssh: Mutex<HashMap<String, SshTarget>>`（`#[cfg(not(target_os = "android"))]`）。`tabs.rs` 的 `open_connection` 在 `ssh_args` 成功后、`register_tunnel` 之前，插入 `SshTarget`。`close` 时一并移除。

- [ ] **Step 5: 改写 `save_guarded`**

把 `validate_source` → `cookies_for_url` → `download_to_path_guarded` 这段替换为：
1. `remote_path_from_url(&source)`；
2. 从 `Connections.ssh` 取该节点的 `SshTarget`，没有则返回 `Err("当前连接不是 SSH 转发，无法下载")`；
3. `ssh_read_args(&target.profile, target.user.as_deref(), target.config.as_deref().and_then(|p| p.to_str()), &path)`；
4. 在目标目录建 `NamedTempFile`，调 `stream_ssh_to_file(args, tmp.as_file_mut())`，`sync_all`，`with_current(|| atomic_rename(...))`。

保留 `safe_file_name`、`next_available`、保存对话框、`with_current` 生命周期检查。删除不再使用的 `validate_source`、`filter_cookies_for_source` 及其辅助函数、`stream_download`、`parse_content_range`、`copy_bounded`、`download`、`download_to_path`、`download_to_path_guarded`，以及它们对应的测试。`reqwest` 依赖若不再被引用则从 `Cargo.toml` 移除。

- [ ] **Step 6: 运行确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 全部 PASS。`cargo build` 无 unused 警告。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/downloads.rs src-tauri/src/tabs.rs src-tauri/src/lib.rs
git commit -m "feat(macos): download remote files over the existing ssh connection"
```

`lib.rs` 只暂存 `Connections` 相关 hunk，用 `git add -p` 确认不含配置导入导出的改动。

---

### Task 4: 在节点 WebView 里挂钩官方 `ui-file-actions`

**Files:**
- Modify: `src/file-actions.ts`
- Modify: `tests/file-actions.test.ts`

**Interfaces:**
- Consumes: 无。
- Produces: `installFileActionsBridge` 内新增对 `root.__ModuleLoader__.load` 的包装。导出一个纯函数 `wrapModuleLoader(loader, targetId, wrapSave)` 便于测试，`installFileActionsBridge` 调它。

- [ ] **Step 1: 写失败测试**

```ts
it('wraps the official file-actions factory so save skips source.read', async () => {
  const { invoke, root } = fixture()
  const registered = new Map<string, { id: string; factory: (require: unknown) => unknown }>()
  const loader = { load(record: { id: string; factory: unknown }) { registered.set(record.id, record as never) } }
  Object.assign(root, { __ModuleLoader__: loader })
  installFileActionsBridge(root)

  // 模拟官方包：apply 时 provide 一个会预读的 actions
  const officialRead = vi.fn(async () => { throw new Error('too-large') })
  const officialFactory = () => ({
    inject: [],
    apply(ctx: { provide: (n: string, v: unknown) => void }) {
      ctx.provide('fileActions', { canSaveAs: true, save: async () => ({ status: 'saved' }), copyImage: async () => {} })
    },
  })
  loader.load({ id: '@deepseek-ai/dsh-client-ui-file-actions', factory: officialFactory })
  const provided: Record<string, unknown> = {}
  const ctx = { provide: (n: string, v: unknown) => { provided[n] = v } }
  registered.get('@deepseek-ai/dsh-client-ui-file-actions')!.factory({})(ctx)
  const actions = provided.fileActions as { save: (s: unknown, m: string) => Promise<unknown>; canSaveAs: boolean }

  const source = { url: 'http://127.0.0.1:40286/api/file?path=/x.apk', name: 'x.apk', read: officialRead }
  await actions.save(source, 'download')
  expect(officialRead).not.toHaveBeenCalled()
  expect(invoke).toHaveBeenCalledWith('save_download', { request: { url: source.url, name: 'x.apk', mode: 'download' } })
  expect(actions.canSaveAs).toBe(true)
})

it('leaves unrelated module ids untouched', () => {
  const { root } = fixture()
  const registered = new Map<string, { id: string; factory: unknown }>()
  installFileActionsBridge(Object.assign(root, { __ModuleLoader__: { load(r: { id: string; factory: unknown }) { registered.set(r.id, r as never) } } }))
  const factory = () => ({ apply() {} })
  root.__ModuleLoader__!.load({ id: 'other-plugin', factory } as never)
  expect(registered.get('other-plugin')!.factory).toBe(factory)
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm exec vitest run tests/file-actions.test.ts`
Expected: FAIL，第一个测试里 `officialRead` 被调用，第二个测试里 `factory` 被包装。

- [ ] **Step 3: 实现 `wrapModuleLoader` 和接线**

`wrapModuleLoader(loader, targetId, wrapSave)`：若 `loader.load` 已被本模块包装过（用一个模块级 `WeakSet` 记录），直接返回。否则替换 `loader.load`：`record.id === targetId` 时把 `record.factory` 包一层，新 factory 调原 factory 得到 `exports`，再把 `exports.apply` 包一层——包内先拦截 `ctx.provide` 拿到官方 `fileActions`，调原 `apply`，然后用 `Object.create(officialActions)` 建对象、覆盖 `save` 为 `wrapSave`，再 `provide('fileActions', patched)`。`record.id` 不等于 `targetId` 时原样传递。

`installFileActionsBridge` 里在创建 `actions` 之后、`root.dshDesktop = …` 之前调 `wrapModuleLoader(root.__ModuleLoader__, '@deepseek-ai/dsh-client-ui-file-actions', (source, mode) => actions.save({ url: source.url, name: source.name, mode }))`。`root.__ModuleLoader__` 不存在时跳过。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm test && pnpm typecheck`
Expected: 全部 PASS。

- [ ] **Step 5: 用真实官方 bundle 做一次本地验证**

Run: `node /tmp/h1-hook-probe-wrap.mjs`
Expected: `"intercepted": true, "readCalls": 0`。这一步不是自动化测试，是手动确认挂钩对真实官方 bundle 有效。探针脚本已存在，若不存在按 Task 说明重建。

- [ ] **Step 6: 提交**

```bash
git add src/file-actions.ts tests/file-actions.test.ts
git commit -m "feat(macos): hook official file-actions to skip blob pre-read"
```

---

### Task 5: 构建安装到 `.13` 并实机验收

**Files:**
- Modify: `docs/operations/macos-native-download.md`（按本设计重写）

**Interfaces:**
- Consumes: Task 1–4。

- [ ] **Step 1: 全量回归**

Run: `pnpm test && pnpm typecheck && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: 全部 PASS。

- [ ] **Step 2: 构建**

Run: `pnpm tauri build --bundles app`，然后 `ditto -c -k --sequesterRsrc --keepParent` 打成 ZIP，记录 ZIP 和可执行文件 SHA-256。

- [ ] **Step 3: 安装到 `.13`**

只读确认主机为 `zcj123vdeMacBook-Air.local`、arm64。正常退出旧客户端，保留完整旧 APP 为 `~/Applications/DSH Client-0.4.1-before-ssh-read-<date>.app`，替换安装，启动后核对可执行文件哈希。不注册自启，不重启 `.13` worker。

- [ ] **Step 4: 实机验收**

在 `.13` 连接 `.6`，对 APK 文件卡点「下载」。预期日志依次出现 `bridge-installed`、`command-start`、`identity-accepted`、`save-start`、`save-ipc-start`，然后文件落到 `~/Downloads`。核对大小和 SHA-256 `56a3ec6e1cadb61b58432c8e1df4068acda27700056dc4b86d7804a90ce9b5e1`。再做一次「另存为…」并取消，确认无临时文件残留。

- [ ] **Step 5: 更新 runbook**

把 `docs/operations/macos-native-download.md` 按本设计重写：删除 HTTP Range、Cookie 筛选、官方包补丁相关内容；保留构建、备份、安装、验收、回滚结构；"与官方包的衔接"一节改为描述 `__ModuleLoader__.load` 挂钩及其失效时的退回行为。

- [ ] **Step 6: 提交**

```bash
git add docs/operations/macos-native-download.md docs/superpowers/specs/2026-11-25-macos-ssh-file-read-design.md docs/superpowers/plans/2026-11-25-macos-ssh-file-read.md
git commit -m "docs: record ssh-based remote file download"
```

同时删除已过时的 `patches/dsh-client-ui-file-actions-0.2.2-rc.3-download-stream/` 目录和 `docs/superpowers/specs/2026-11-23-macos-native-download-design.md`，一并提交。
