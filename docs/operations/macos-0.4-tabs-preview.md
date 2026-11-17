# DSH Client 0.4.0 macOS 多标签预览

本预览仅 macOS Apple Silicon (arm64)，最低 macOS 14；未签名、未公证。只构建 macOS，其他平台待用户确认体验后再打包。保持纯客户端边界，不需要改或重启 DSH Host/worker。

## 使用

顶部固定“节点管理”标签；点 `＋` 可选已保存节点或新增节点。节点已打开时直接切换并保留原页面与输入；“连接中”节点不可重复开；失败节点可点标签/管理页“打开 / 重试”重新连接。每节点只占一个标签。点标签 `×` 仅关该节点标签、回收对应SSH转发并保留节点记录及其独立浏览数据；管理标签永不关闭。

红色关闭按钮只隐藏客户端窗口，保留所有已开节点连接；从Dock点客户端图标恢复。macOS菜单“节点管理”`⌘1`、“新建连接标签”`⌘T`、“关闭当前标签”`⌘W`。`⌘Q` 完全退出并回收所有SSH转发。新建节点的入口优先级 LAN→WG 与0.3.0一致。

## 升级与回滚

安装前关闭旧版客户端（不是 DSH worker），保留旧 `DSH Client.app` 作为回滚备份。安装预览版后首次打开时，已有节点与本机管理数据应继续可见；升级不覆盖节点外部服务。未签名包若被Gatekeeper拦截，只对本次可信预览包用Finder右键打开。

回滚时先退出0.4.0，保留失败APP供检查，再恢复升级前APP并打开；不要删除客户端数据、节点 Profile、凭据、Cookie或DSH会话。

## 构建验证与实际验收范围

- 包：`DSH-Client-0.4.0-macos-arm64-tabs-preview.zip`，2,212,639字节；ZIP SHA-256 `4705187b04aa76acb3ea436a1e8937104257f12eeec78f78e9e27e1e216a3a05`。
- APP内 `dsh-client` SHA-256 `b03d3a896340ae6bf7dfd4f5fde3b221d596cf6ae624020b0a656f2189034374`，bundle ID `com.zcj.dshclient`、版本 `0.4.0`、仅 `arm64`。
- 前端 `pnpm test`：8个文件、36项通过；`pnpm typecheck` 和 `pnpm build` 通过。Rust library：26项通过、2项既有显式忽略（仅 `.13` SSH live route 与其 fixture launcher）。另有原生会话代际、关闭取消、来源权限和固定56logicalpx布局测试。
- 独立只读审查未发现 Critical / Important；标签关闭状态现同步回节点管理列表。非macOS路径使用稳定窗口API，但本次没有构建或发布其他平台。
- **`.13` 实机安装（2026-11-17）**：主机 `zcj123vdeMacBook-Air.local`，arm64。安装路径 `~/Applications/DSH Client.app`；0.3.0旧APP完整备份 `~/Applications/DSH Client-0.3.0-backup-20261117.app`。新APP Plist版本0.4.0，bundle ID保持 `com.zcj.dshclient`，运行PID `99714`，安装二进制哈希与本机最终包内 `b03d3a896340ae6bf7dfd4f5fde3b221d596cf6ae624020b0a656f2189034374` 一致。第一次安装的0.4.0预览包另保留在 `~/Applications/DSH Client-0.4.0-intermediate-20261117.app`（辅助回滚）；最终包更新前已正常退出首个客户端，检查无活动SSH转发，然后重新安装并启动。更新前后 `.13` 的 `127.0.0.1:3080` DSH worker均为PID `91076`；无服务重启、无自启注册。
- **用户界面反馈待 `.13` 实际试用**：构建机系统屏幕截图全黑、原生窗口无障碍树不可寻址；尚未证明新版界面显示、点击或窗口生命周期效果。安装与进程启动有远端证据，但登录、tab保留、Dock恢复、SSH重试和业务体验仍未验证。若不满意，退出0.4.0并用上述备份完整回滚。
