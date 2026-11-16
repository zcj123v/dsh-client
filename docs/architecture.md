# Architecture

## Components

```text
Shell UI (React)
  ├─ ConnectionStore: local metadata only
  ├─ ConnectionManager: URL/state/port/reconnect boundary
  └─ WebviewWindow per connection
       ├─ direct: existing HTTPS/host entry
       └─ ssh-forward: local loopback -> OpenSSH -> remote 127.0.0.1:3080
```

DSH Web UI、Client Connection、RPC、WebSocket、会话和插件均由目标节点提供。客户端不实现 `/api/remote.mux`，不复刻 DSH session API，也不使用 Fleet Bridge 代理普通对话。

## Connection record

```text
id, name, mode, url?
sshProfile?, sshUser?, remoteHost?, remotePort
partition, createdAt, lastOpenedAt?
```

`partition` 只用于 WebView 存储隔离。token、Cookie、Basic Auth 和私钥不属于记录。

## Direct

direct 只接受 `http://` 或 `https://` URL，v1 默认要求已有 HTTPS/域名入口。当前 DSH 节点不因客户端而改成非 loopback 监听。访问受目标 DSH 自身的 Host/Origin 栅栏和 token-to-cookie 认证保护。

## SSH-forward

桌面端从系统 SSH 配置读取 Host 别名、跳板、代理和 known_hosts。Rust 端只接收已经拆分好的参数数组，固定参数包括 `-N`、`ExitOnForwardFailure=yes`、心跳和 `-L 127.0.0.1:<local>:127.0.0.1:<remote>`。客户端退出时清理所有子进程。

## v1 multi-window

桌面 v1 每个连接使用稳定的 Tauri `WebviewWindow`，连接列表承担标签/窗口入口；同一窗口内真标签留到后续 multiwebview 阶段。Android 使用连接列表+当前 WebView。不同连接使用独立分区，删除连接时由后续平台适配清理对应分区。
