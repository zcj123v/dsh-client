# 架构

本地 React 壳负责连接列表、一次性认证输入、连接元数据和打开状态。Rust command 负责校验来源、系统 OpenSSH、就绪探测和 WebView 生命周期。目标节点提供全部 DSH Web、RPC、WebSocket、会话和执行环境。

## 连接与原生边界

持久化字段采用白名单：id/name/mode/url、sshProfile/sshUser/sshConfig、remotePort、partition、createdAt/lastOpenedAt。认证正文不属于连接记录；无效或含凭据的历史记录在恢复时丢弃。

仅本地 main 窗口拥有 Tauri capability。原生 command 再检查调用窗口 label 和 origin。远端 dsh-* 窗口没有 capability，不允许导航到本地壳 origin。

## 窗口与存储

macOS 14+ 使用连接 ID SHA-256 的前 16 字节作为 WKWebView data store 标识；Windows/Linux 使用独立 data directory。不同连接拥有不同 Cookie/LocalStorage 容器。退出保留数据；删除连接请求系统异步清理，随后废弃 ID。系统清理 API 无同步完成保证。

Android 使用原生普通 Activity + WebViewCompat.setProfile，在任何设置/导航前选定连接 Profile；不使用 Tauri 的桌面分区选项，也不向远端注入 IPC。系统 WebView 不支持 MULTI_PROFILE 时拒绝打开。删除已加载 Profile 用持久墓碑，下次进程启动、尚未加载时清理；详见 Android 操作文档。arm64 APK 已编译，设备隔离和真实 DSH 业务仍需验收。

## SSH 转发

Rust 独立校验 Host/用户/端口；系统 ssh 使用参数数组、BatchMode、StrictHostKeyChecking、ExitOnForwardFailure 和保活。配置选择只允许系统默认、~/.ssh/dsh-fleet-config 或显式不读取配置（Unix /dev/null、Windows NUL）。目标始终为远端 loopback。

端口按 ID 稳定选取，先检查可绑定再启动 ssh；绑定移交存在 OS/OpenSSH 固有的极短竞争窗口，转发失败会回收子进程，不降级到其他服务。HTTP 就绪探测只是传输可达性，预登录 401/403 不等于认证成功；404/502 等不能通过探测。

SSH 在 spawn 时就进入待清理注册表。窗口销毁、删除连接、打开失败和最终退出均 kill/wait。连接期间退出同样清理 pending 进程。重连由用户关闭旧窗口后重新打开，DSH 页面的 RPC/WS 自动重连沿用目标上游行为；尚未实现客户端自动重建隧道的退避循环。

## 认证与导航

一次性 token 只参与本次根 URL 导航，加载完成后移除当前历史项中的 token 查询参数。不保存到连接记录、不输出到日志。已有 Cookie 用于后续打开。HTTPS 依赖系统 WebView 的信任链和名称校验，不绕过自签名/过期/不匹配证书，不提供自定义证书 pinning。

主导航限制为当前 origin；外链通过系统浏览器，含明显凭据参数或非 HTTP(S) 的跳转拒绝。相同 origin 的 popup 转到当前窗口，复杂 OAuth/popup 流程仍需实机兼容验证。
