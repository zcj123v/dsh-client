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

当前安装记录（2026-11-25，v2）：

```text
ZIP:   DSH-Client-0.4.1-macos-ssh-read-v2-arm64.zip
ZIP SHA-256:        95e233aad63683eb9d8a7e537f42ace60914215c6af3210e471cae37bfb1d3b7
二进制 SHA-256:      d225b14177b3b3de4307224217dce73471436bde0c7baf85696b4f77767b8f00
（版本 0.4.1，arm64，adhoc 未签名；含 45de60a 二次 provide 修复）
回滚备份:           ~/Applications/DSH Client-0.4.1-before-ssh-read-v2-20261125.app（0.4.1 cookie-fix 版，83ad219…）
历史:               v1 构建（490e8cf2…，ZIP f76c825f…）因二次 provide 破坏整页启动，已停放在
                     ~/Applications/DSH Client-0.4.1-ssh-read-broken-20261125.app，勿再安装；
                     v1 备份 DSH Client-0.4.1-before-ssh-read-20261125.app 与 v2 备份内容相同
更早备份链:         DSH Client-0.4.1-before-cookie-fix-20261124.app 等
```

## 5. 实机验收

在 `.13` 连接真实 `.6` DSH，不用公开网页或编译结果替代：

1. 打开节点页面后确认日志出现 `[dsh-download] bridge-installed`；没有该记录不得声称原生链路可用。
2. 非图片文件卡（本轮用 257 MB debug APK）菜单点「另存为…」，系统对话框选「下载」目录保存。
3. 日志阶段应依次出现（2026-11-25 实测顺序。网页侧阶段经 `document.title` 通道由 Rust 轮询回显，
   `save-ipc-start` 落盘位置滞后到 `dialog-start` 之后、`save-request` 常被下一个阶段覆盖而缺失，均属通道特性、不算乱序错误）：
   `bridge-installed` → `command-start` → `identity-accepted` → `save-start` → `dialog-start` →
   `save-ipc-start` → `dialog-selected` → `transfer-start` → `commit-complete` → `command-saved` → `save-ipc-saved`。
4. 落盘核对：位置（对话框所选目录）、字节数与 `.6` 源文件一致、SHA-256 与验收当下 `.6` 源文件实测值一致。源文件重建后哈希会变：`56a3ec6e…` 是 11-23 旧 APK 的记录，11-24 重建后为 `344daffb…`；每轮以实测为准。
5. 取消测试：再点一次「另存为…」并取消；取消发生在系统对话框阶段、先于临时文件创建，预期不产生任何 `.dsh-download-*` 残留，验收时确认所选目录确无残留。覆盖语义分模式：`download` 模式同名自动加序号、从不覆盖既有文件；`saveAs` 模式尊重系统对话框的替换确认——用户在对话框里确认替换后整体替换既有文件（2026-11-25 实测：同名确认替换了 11-18 的旧 APK）。
6. TCC 注意：SSH 会话对 `~/Downloads` 的目录列举被 macOS TCC 拒绝（`ls` / `find` 报 Operation not permitted），但按路径直接读文件可用（2026-11-25 实测：SSH 会话内 `shasum -a 256 ~/Downloads/<文件名>` 成功）。落盘哈希可在 SSH 会话直接算；残留检查需要目录列举，请用户在 `.13` 本机执行 `ls -a ~/Downloads | grep dsh-download`（期望无输出）回传结果，或先在系统设置为 sshd 授予「下载文件夹」访问。

阶段排障：

- `bridge-missing` / `ipc-unavailable`：节点 WebView 桥接未安装或 IPC 不可用。
- `save-ipc-acl-rejected`：IPC ACL 拒绝（调用方不是已注册节点 WebView）。
- `identity-window-rejected` / `identity-cancelled` / `identity-origin-mismatch`：窗口、generation 或 origin 校验失败。
- `command-error`（含「远端读取失败」）：远端 `cat` 非零退出——文件不存在、权限不足、SSH 配置或网络问题；错误信息不透出远端 stderr。
- 「当前连接不是 SSH 转发，无法下载」：直接模式连接发起的保存。
- `dialog-cancelled` / `command-cancelled`：用户在系统对话框取消（预期路径）。
- `owner-stale` / `owner-origin-changed`：标签关闭或替换后旧请求失效（预期保护，不应再落盘）。

本轮验收记录（2026-11-25）：**部分有证据，完整验收记录未闭环**。v2 已按 §4 安装到 `.13`；14:5x 经受限 SSH 只读复核，主机为 `zcj123vdeMacBook-Air.local` / arm64，当前二进制 SHA-256 为 `d225b14177b3b3de4307224217dce73471436bde0c7baf85696b4f77767b8f00`，与 v2 安装记录一致，客户端进程在运行。本文 §5 已补日志阶段顺序、同名覆盖和 TCC 的部分实测结果，但源文件/落盘文件的逐组字节数与 SHA-256、取消后的残留检查等尚未形成完整逐项记录，因此不把 §5 判为全部通过。v1 因二次 provide 破坏页面启动，已隔离且不得再安装。配置导入导出工作线入库后，原「提交树缺 dialog 插件注册」的注意事项已按约定删除；`pnpm test` / `pnpm typecheck` / `pnpm build`、Rust lib 测试与 Android 策略测试均通过，但编译与测试不等于实机验收。待办：按 §5 补齐落盘哈希、取消残留等逐项证据后，再把本段改为验收完成。

## 6. 回滚

1. 正常退出客户端（同 §4.3），保留失败版本供排查。
2. `mv ~/Applications/"DSH Client-0.4.1-before-ssh-read-v2-20261125.app" ~/Applications/"DSH Client.app"`。
3. 启动旧版并核对二进制 SHA-256 与备份记录一致；不删除用户节点存储或 DSH 凭据。
4. 用户已裁定 `.13` 不回滚到 stable 旧版：保留当前预览，直至下一个构建替换。

## 7. 升级与退役注意

- DSH 升级后官方 UI 包形态若变化，挂钩按 §2 自动退回官方行为，页面其余功能不受影响；只需检查 `src/file-actions.ts` 一处。
- 上游若原生支持桌面大文件流式保存，优先采用上游实现并移除挂钩。
- 诊断日志与本文档均不得固化 token、Cookie 值、私钥路径或生产密钥。
- 编译与测试通过不等于验收通过：每次发布以 §5 实机验收的落盘哈希核对为准。
