# macOS 经 SSH 读取远端文件的设计

日期：2026-11-25
状态：待用户批准
取代：`2026-11-23-macos-native-download-design.md` 中的 HTTP Range 下载路径

## 背景与原因

`.13` 的 DSH Client 连接 `.6` 的 DSH 时，用户希望把远端附件保存到 `.13` 本机。

上一版设计让客户端用 HTTP Range 请求 `/api/file`。实测失败，原因是 DSH 的 `/api/file` 有两处服务端限制，客户端无法绕过：

- 不支持 `Range` 请求，每次返回整个文件；
- 文件大小上限默认 50 MB（`DEFAULT_MAX_IMAGE_BYTES`），超限返回 `413`。

`.6` 的官方 `@deepseek-ai/dsh-client-ui-file-actions` 包已恢复为未修改的上游版本，本设计不再修改任何 DSH 官方模块。

## 目标

- 在 `.13` 上把 `.6` 的文件保存到本机，支持「下载」和「另存为…」。
- 支持 257 MB 的 APK，不整体读入内存。
- 不修改 DSH 官方模块。

## 信任模型（用户裁定，2026-11-25）

本软件是用户自用的内部工具，只有用户本人使用。安全目标是**从外部足够安全**，不在客户端内部假设网页或客户端自身被攻破。

由此简化：

- 不做 `realpath` 往返和前缀比对。路径由网页传入，客户端直接交给 SSH 读取。
- 不做路径白名单、命令名白名单或工作区根目录读取。
- SSH 调用仍用参数数组、路径经 `--` 传入，这是避免路径含空格或特殊字符时出错，不是安全措施。
- 保留外部边界：SSH 隧道仍绑定回环、DSH 仍有登录认证、`.6` 的 `authorized_keys` 不因本设计改动。

## 非目标

- 不提供远端写文件能力。
- 不改变现有 SSH 隧道的建立方式和认证方式。
- 不限制可读取的远端路径。
- 不为其他平台（Windows/Linux/Android）实现下载。

## 约束与已知事实

- `.13` 上的 `~/.ssh/id_ed25519` 在 `.6` 的 `authorized_keys` 第 5 条，无任何限制。客户端已用它建立 SSH 隧道，这是既有状态。
- `.6` 的 fleet 节点已登记工作区根目录 `/Users/zhangchengjie/Workspace`，来源是 `worker.env` 的 `DSH_FLEET_NODE_WORKSPACE_ROOTS`。
- DSH 页面传给下载功能的路径是绝对路径（`fileMediaUrl` 在 `absolutePath` 可用时构造 `api/file?path=<绝对路径>`）。
- 本设计的路径限制在客户端侧执行，不是密码学边界。客户端自身被攻破时，限制无效。

## 方案比较

| 方案 | 做法 | 结论 |
|---|---|---|
| HTTP Range | 客户端请求 `/api/file` | 排除：服务端不支持 Range，50 MB 上限 |
| **经 SSH 直接读** | 客户端用已有 SSH 配置执行 `cat -- <path>`，流式写入本地 | 采用 |

## 设计

### 读取流程

1. 网页通过 `window.dshDesktop.fileActions.save({url, name, mode})` 发起请求，`url` 是 `http://127.0.0.1:<port>/api/file?path=<绝对路径>`。
2. Rust 从 `url` 解析出 `path`。
3. Rust 用该连接已有的 SSH 配置执行 `cat -- <path>`，stdout 流式写入本地临时文件。
4. 写完后原子重命名为目标文件名。

### SSH 调用

- 复用该连接已有的 SSH 配置（`sshConfig`、`sshProfile`、`sshUser`），不新增密钥。
- 用 `Command::new("ssh")` 加参数数组，路径通过 `--` 之后的单独参数传入。
- 直接模式（非 SSH 转发）的连接没有 SSH 配置，不支持下载，返回明确错误。

### 本地保存

- 「下载」保存到系统下载目录，重名时追加序号，不覆盖。
- 「另存为…」通过系统对话框选择位置。
- 临时文件与目标文件同目录，成功后 `rename`，失败或取消时删除。

### 与官方 UI 包的衔接

`.6` 的官方包未修改，网页会先把文件读成 Blob 再调用桥接。超过 50 MB 的文件在这一步失败，桥接收不到请求。

客户端在节点 WebView 里包装 `window.__ModuleLoader__.load`，只对 `id === '@deepseek-ai/dsh-client-ui-file-actions'` 的记录生效：把它的 `factory` 包一层，返回的 `apply` 先照常执行，再覆盖它 `provide('fileActions', …)` 的结果，把 `save` 换成"直接把原始 URL 交给 Rust、不调用 `source.read`"的版本。菜单、图标、locale 等副作用全部保留。

已用真实官方 bundle 在本地验证：`source.read` 零次调用，`url`、`name`、`mode` 原样传递。

依赖的官方形态只有两点：`factory` 返回 `{apply}`，`apply` 通过 `ctx.provide('fileActions', …)` 注册。若 DSH 升级后这两点变化，挂钩失效，下载退回官方行为（小文件可用、大文件报错），不会影响其他功能。

## 可更新性（用户要求）

- 不修改任何 DSH 官方文件；DSH 升级后无需重放补丁。
- 挂钩逻辑集中在客户端 `src/file-actions.ts` 一处，官方形态变化时只改这一处。
- 挂钩失效时退回官方行为，不破坏页面其他功能。
- 客户端自身目前没有自动更新机制，每次手动替换 APP。自动更新作为独立事项，在下载功能完成后另行处理。

## 验证

- 单元测试：从 URL 解析路径、SSH 参数构造、流式写入与原子重命名。
- 实机验收：在 `.13` 上连接 `.6`，下载 257 MB APK，核对落盘位置、大小、SHA-256 `56a3ec6e1cadb61b58432c8e1df4068acda27700056dc4b86d7804a90ce9b5e1`。

## 待确认事项

无。挂钩点已验证，`.13` 预览版按用户要求不回滚。

## 既有材料

- `.6` 官方包备份：`~/.local/share/dsh-fleet/runtime-backups/dsh-client-ui-file-actions-0.2.2-rc.3-before-download-stream`
- `.13` 回滚备份：`~/Applications/DSH Client-0.4.1-before-cookie-fix-20261124.app`
- 已写但尚未提交的 runbook：`docs/operations/macos-native-download.md`，需按本设计更新。
