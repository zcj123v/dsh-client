# macOS 经 SSH 远端文件下载 Runbook

适用对象：DSH Client macOS 版，安装在 `.13`（`zcj123vdeMacBook-Air.local`，arm64）。目标是在 `.13` 连接 `.6` 的 DSH 时，把远端文件保存到本机：非图片文件卡菜单的「另存为…」走系统保存对话框；「下载」落到系统下载目录并自动避让重名。已用 257 MB debug APK 实测。

> 本能力只覆盖 macOS 客户端与 SSH 转发连接。直接模式（非 SSH 转发）连接不支持下载，返回明确错误。全程不修改任何 DSH 官方文件，`.6` 侧无需任何配合改动。

## 1. 实现边界与源码位置

客户端源码：

- `src-tauri/src/downloads.rs`
  - `remote_path_from_url`：从 `api/file?path=<绝对路径>` 解析远端路径，空值或相对路径报「远端路径无效」。
  - `ssh_read_args`：构造 `ssh [-F config] -o BatchMode=yes -o StrictHostKeyChecking=yes [-l user] <profile> -- cat -- <path>`；`profile`/`user` 沿用 `policy::ssh_args` 的 safe 校验。路径作为 `--` 之后的独立参数传递，只为避免路径含空格或特殊字符时出错。
  - `stream_ssh_to_file`：spawn `ssh`，stdout 以 64 KiB 缓冲流式写入本地临时文件（不随文件大小增长内存），远端非零退出报「远端读取失败」，不透出远端 stderr。
  - 保存对话框、下载目录、重名避让（不覆盖既有文件）、同目录 `.dsh-download-*` 临时文件、`sync_all` 后原子 `renameat`（`RENAME_EXCL`）提交；失败或取消删除临时文件。
  - IPC ACL：`save_download` 只接受已注册节点 WebView 的调用（origin、窗口、generation 校验）；标签关闭、替换或新 generation 使 pending 保存失效并回收。
- `src/file-actions.ts`：节点 WebView 的 `window.dshDesktop.fileActions` 桥接，及对官方 UI 包的挂钩（见 §2）。
- `src-tauri/src/tabs.rs`、`src-tauri/src/lib.rs`：`Connections` 记录每条连接的 `SshTarget { profile, user, config }`，连接建立时写入、关闭时移除；WebView 注入与标签生命周期。
- 测试：`tests/file-actions.test.ts`（桥接与挂钩行为）、`src-tauri/src/downloads.rs` `mod tests`（路径解析、SSH 参数、流式写入与非零退出、原子提交与 owner 失效）。

信任模型（用户裁定，2026-11-25）：用户自用内部工具，安全目标是「从外部足够安全」，不在客户端内部假设网页或客户端自身被攻破。因此不做 `realpath` 往返、路径白名单、命令名白名单或工作区根目录限制；SSH 调用固定为参数数组加 `cat -- <path>`，不提供任意命令接口。外部边界保留：SSH 隧道仍绑定回环、DSH 侧登录认证不变、`.6` 的 `authorized_keys` 不因本能力改动。

直接模式（无 SSH 转发）连接发起保存时报「当前连接不是 SSH 转发，无法下载」。

## 2. 与官方包的衔接

`.6` 的官方 `@deepseek-ai/dsh-client-ui-file-actions` 保持未修改。官方 `save` 的固有行为是先把文件 `source.read` 成 Blob，超过 `/api/file` 的 50 MB 上限会在这一步失败——客户端挂钩的作用就是绕过这次预读：

- 桥接初始化脚本在节点 WebView 里把 `window.__ModuleLoader__` 属性替换为 accessor：页面脚本在桥接之后才安装 loader 时，赋进来的 loader 会先被包装再收下；安装时已存在的 loader 直接包装。
- 包装只对 `record.id === '@deepseek-ai/dsh-client-ui-file-actions'` 的注册记录生效：把该记录的 `factory` 包一层——原 `factory` 照常产出 exports；`exports.apply` 再包一层：先拦截 `ctx.provide('fileActions', …)` 拿到官方 actions，原 `apply` 照常执行（菜单、图标、locale 等副作用全部保留），然后就地把官方 `save` 换成「把原始 `url`、`name`、`mode` 直接交给 `save_download` IPC、不调用 `source.read`」的版本。官方 `apply` 把同一个 actions 对象交给菜单闭包，就地替换同时改写「注册进容器的服务」和「菜单闭包实际调用的入口」。
- **不得对同一服务名二次 `provide`**：Cordis 对重复注册同名服务直接抛错，会使整页插件加载失败（2026-11-25 在 `.13` 实机暴露：`service fileActions has been registered`，已回滚修复）。挂钩只做就地替换，不重新注册服务。
- `loader.load` 本身也用 accessor 包装：页面模块系统 `create()` 会整体替换 `target.load`（队列模式切实时模式），后替换进来的函数同样被包装；同一条注册记录只包一次。
- **失效退回**：挂钩依赖的官方形态只有两点——`factory` 返回 `{apply}`，`apply` 经 `ctx.provide('fileActions', …)` 注册含 `save` 的对象。DSH 升级后若这两点变化，挂钩静默让位，下载退回官方行为（小文件可用、大文件报错），页面其他功能不受影响。挂钩逻辑集中在 `src/file-actions.ts` 一处，官方形态变化时只改这一处；不需要在任何机器上重放补丁。

## 3. 构建与离线验证

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # cargo 不在默认 PATH
pnpm test && pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri build --bundles app
ditto -c -k --sequesterRsrc --keepParent "src-tauri/target/release/bundle/macos/DSH Client.app" "DSH-Client-<版本>-<标签>-arm64.zip"
shasum -a 256 "DSH-Client-<版本>-<标签>-arm64.zip" \
  "src-tauri/target/release/bundle/macos/DSH Client.app/Contents/MacOS/dsh-client"
```

- 同时记录 ZIP 与 `Contents/MacOS/dsh-client` 两个 SHA-256；发布的是未签名 adhoc 测试包，必须写明版本、架构与两个哈希，不得声称签名/公证。
- **安装前门禁（2026-11-25 事故后新增）**：用 `.6` runtime 的真实官方 `lib/client.js` 跑一次「Cordis 一致」探针——mock 的 `ctx.provide` 必须对重复服务名抛错（与真实容器一致），经 Rust 注入变换执行当前 `src/file-actions.ts`，确认：官方 `factory` 被包装、`apply` 不抛错、`fileActions` 恰好注册一次、菜单入口实际调用 `save_download` 且 `source.read` 零调用。探针抛错或出现二次注册即停止，不得安装。单测与宽松 mock 探针拦不住这类容器约束（本轮实机教训）。
- 构建前确认 `.6` 上用于验收的源文件存在并实测哈希（APK 重建后哈希会变，以验收当下为准）。

## 4. 备份与安装（`.13`）

1. 只读核实目标主机：`ssh -F ~/.ssh/dsh-fleet-config dsh-air 'hostname; uname -m'`，必须返回 `zcj123vdeMacBook-Air.local` / `arm64`，不符即停。
2. 传包：`scp -F ~/.ssh/dsh-fleet-config <zip> dsh-air:/tmp/`，远端 `shasum -a 256` 与本地一致才继续。
3. 正常退出旧客户端：`osascript -e 'tell application "DSH Client" to quit'`，轮询 `pgrep -x dsh-client` 直到进程退出（SSH 转发随之回收）。
4. 备份旧 APP：`mv ~/Applications/"DSH Client.app" ~/Applications/"DSH Client-0.4.1-before-ssh-read-<YYYYMMDD>.app"`；同日再次安装时加 `-v2`、`-v3` 序号，不得复用已占用的备份名。
5. 解压安装：`ditto -x -k --sequesterRsrc /tmp/<zip> ~/Applications/`。
6. 核对安装哈希：`shasum -a 256 ~/Applications/"DSH Client.app/Contents/MacOS/dsh-client"` 与构建记录一致，不一致即停。
7. 启动（直接运行二进制，stderr 落诊断日志）：

   ```bash
   nohup ~/Applications/"DSH Client.app/Contents/MacOS/dsh-client" >/dev/null 2>/tmp/dsh-download-diagnostic.log &
   ```

   日志行形如 `[dsh-download] <stage>`，阶段名为固定集合，不含 URL、Cookie、token、文件内容或路径。
8. 不注册自启；不重启、不干预 `.13` 上任何 DSH worker；不使用 `launchctl submit`。

当前安装记录（2026-11-25）：

```text
ZIP:   DSH-Client-0.4.1-macos-ssh-read-arm64.zip
ZIP SHA-256:        f76c825f916f7c768414a7a97c79b759bf5334b7d775a44b9392f2ff49624efb
二进制 SHA-256:      490e8cf2a489e69e5c0ad18400a093ab3d4874a26c6ec4c5316a632c04f463fb
（版本 0.4.1，arm64，adhoc 未签名）
回滚备份:           ~/Applications/DSH Client-0.4.1-before-ssh-read-20261125.app
更早备份链:         DSH Client-0.4.1-before-cookie-fix-20261124.app 等
```

## 5. 实机验收

在 `.13` 连接真实 `.6` DSH，不用公开网页或编译结果替代：

1. 打开节点页面后确认日志出现 `[dsh-download] bridge-installed`；没有该记录不得声称原生链路可用。
2. 非图片文件卡（本轮用 257 MB debug APK）菜单点「另存为…」，系统对话框选「下载」目录保存。
3. 日志阶段应依次出现（真实代码顺序，`save-ipc-start` 是网页侧阶段、先于 Rust 命令）：
   `bridge-installed` → `save-request` → `save-ipc-start` → `command-start` → `identity-accepted` → `save-start` → `dialog-start` → `dialog-selected` → `transfer-start` → `commit-complete` → `command-saved` → `save-ipc-saved`。
4. 落盘核对：位置（对话框所选目录）、字节数与 `.6` 源文件一致、SHA-256 与验收当下 `.6` 源文件实测值一致。源文件重建后哈希会变：`56a3ec6e…` 是 11-23 旧 APK 的记录，11-24 重建后为 `344daffb…`；每轮以实测为准。
5. 取消测试：再点一次「另存为…」并取消，确认所选目录无 `.dsh-download-*` 临时文件残留；同名再次保存不覆盖既有文件（自动加序号）。
6. TCC 注意：SSH 会话读 `~/Downloads` 受 macOS TCC 限制（`ls` 报 Operation not permitted）。核验时若屏幕出现『"sshd"想访问"下载"文件夹』弹窗，请用户点「允许」；若始终被拒，请用户在 `.13` 本机执行 `shasum -a 256 ~/Downloads/<文件名>` 回传结果，或先在系统设置授权。

阶段排障：

- `bridge-missing` / `ipc-unavailable`：节点 WebView 桥接未安装或 IPC 不可用。
- `save-ipc-acl-rejected`：IPC ACL 拒绝（调用方不是已注册节点 WebView）。
- `identity-window-rejected` / `identity-cancelled` / `identity-origin-mismatch`：窗口、generation 或 origin 校验失败。
- `command-error`（含「远端读取失败」）：远端 `cat` 非零退出——文件不存在、权限不足、SSH 配置或网络问题；错误信息不透出远端 stderr。
- 「当前连接不是 SSH 转发，无法下载」：直接模式连接发起的保存。
- `dialog-cancelled` / `command-cancelled`：用户在系统对话框取消（预期路径）。
- `owner-stale` / `owner-origin-changed`：标签关闭或替换后旧请求失效（预期保护，不应再落盘）。

本轮验收记录（2026-11-25）：**未完成**。ssh-read 版已按 §4 安装到 `.13`（哈希见上），但 §5 的落盘验收（日志阶段链、字节数与 SHA-256 核对、取消与同名保存测试）未见执行记录；对应会话在写本文档时被上游 API 故障中断。收尾提交（「配置导入导出」工作线 + 本文档）时只复跑了 `pnpm test` / `pnpm typecheck` / `pnpm build`、`cargo test --manifest-path src-tauri/Cargo.toml --lib` 与 Android 策略测试（均通过；`lifecycle_tests::lost_forward_recovers_in_place` 在并发首跑时偶发失败，单测与全量重跑通过），编译与测试通过不等于验收通过。配置导入导出工作线入库后，原「提交树缺 dialog 插件注册」的注意事项已按其约定删除。待办：在 `.13` 按 §5 完成实机验收并补记哈希；完成前不得声称原生下载 / SSH 读文件已验收。

## 6. 回滚

1. 正常退出客户端（同 §4.3），保留失败版本供排查。
2. `mv ~/Applications/"DSH Client-0.4.1-before-ssh-read-20261125.app" ~/Applications/"DSH Client.app"`。
3. 启动旧版并核对二进制 SHA-256 与备份记录一致；不删除用户节点存储或 DSH 凭据。
4. 用户已裁定 `.13` 不回滚到 stable 旧版：保留当前预览，直至下一个构建替换。

## 7. 升级与退役注意

- DSH 升级后官方 UI 包形态若变化，挂钩按 §2 自动退回官方行为，页面其余功能不受影响；只需检查 `src/file-actions.ts` 一处。
- 上游若原生支持桌面大文件流式保存，优先采用上游实现并移除挂钩。
- 诊断日志与本文档均不得固化 token、Cookie 值、私钥路径或生产密钥。
- 编译与测试通过不等于验收通过：每次发布以 §5 实机验收的落盘哈希核对为准。
