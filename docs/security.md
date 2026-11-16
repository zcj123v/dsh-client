# Security

## Threat model

客户端连接个人或受控网络内已经运行的 DSH。客户端不成为新的执行体，不保存模型密钥，也不把 DSH API 暴露到公网。

## Rules

1. direct 优先 HTTPS；HTTP 只允许用户明确选择的受控 WireGuard/loopback 环境。
2. 当前节点保持 loopback；跨机完整 UI 使用 SSH LocalForward。已有 HTTPS 网关作为可选 direct，不通过伪造 Host/Origin 绕过 DSH 栅栏。
3. token 只在用户首次打开根 URL 时手动粘贴，客户端不读取、不记录、不进入错误消息。Basic Auth 由 WebView 表单处理。
4. Cookie 保存在 WebView 的连接分区；退出保留，删除连接清理。认证失效后重新粘贴 token。
5. 远端页面不获得 Tauri IPC capability；外部导航不允许脱离当前连接 origin，外部链接交给系统浏览器。
6. SSH 只调用固定 `ssh` executable 和参数数组。Host 别名与用户字段拒绝 shell 元字符；不拼接 shell 命令，不复制私钥。
7. SSH 转发只监听 `127.0.0.1`，使用随机端口，设置 `ExitOnForwardFailure` 和保活；退出时 kill/wait 全部子进程。
8. 日志和诊断不得输出 token、Cookie、Authorization、Basic Auth、私钥内容或完整环境变量。

## Known limitations

- Tauri WebView 的证书指纹 pinning 尚未在 v1 实现；自签名/名称不匹配证书仍由系统 WebView 处理，不静默绕过。
- Android 首版不提供内置 SSH；需要 direct HTTPS 或设备已有 VPN。
- 单窗口多标签未纳入 v1 验收；当前是每连接独立窗口。
