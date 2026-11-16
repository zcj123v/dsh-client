# MacBook Air .13

目标：`dsh-air`（使用 `~/.ssh/dsh-fleet-config`），已核实 arm64、macOS 27.0.1。

## 安装前

从本机完成 `pnpm test`、`pnpm typecheck`、`pnpm build`、`cargo check` 和 macOS arm64 Tauri build 后，记录产物 SHA-256。安装只写用户目录，不改 DSH worker、profile、LaunchAgent 或自启。

## 传输与安装

```bash
scp -F ~/.ssh/dsh-fleet-config <DSH_CLIENT_DMG> dsh-air:/tmp/DSH-Client-arm64.dmg
ssh -F ~/.ssh/dsh-fleet-config dsh-air 'shasum -a 256 /tmp/DSH-Client-arm64.dmg'
```

在 `.13` 图形会话中打开 DMG，把 `DSH Client.app` 拖到用户 `~/Applications`（如目录不存在先创建），首次启动选择“打开”。本轮未签名，不执行系统级安装，不注册 LaunchAgent。

## 验收

1. 添加 direct `http://127.0.0.1:3080/` 连接，打开目标 DSH 根页面并手动粘贴 token。
2. 验证会话列表、流式输出、附件/文件入口和断线后的页面重连。
3. 添加一个 SSH-forward 连接，填写 `~/.ssh/dsh-fleet-config` 的 Host 别名和远端 DSH 端口，确认本机 loopback 转发可用。
4. 打开两个连接窗口，确认认证和本地存储不串线。
5. 关闭客户端后检查没有残留由客户端启动的 `ssh -N -L` 进程；不要重启 DSH worker。

## 回滚

```bash
rm -rf "$HOME/Applications/DSH Client.app"
rm -rf "$HOME/Library/Application Support/com.zcj.dshclient"
rm -rf "$HOME/Library/Caches/com.zcj.dshclient"
rm -f /tmp/DSH-Client-arm64.dmg
```

回滚只删除客户端文件和分区，不删除 `.13` 的 DSH 数据、凭据或会话。
