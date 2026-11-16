# MacBook Air .13 安装与测试

目标 dsh-air，SSH 配置 ~/.ssh/dsh-fleet-config，用户 zcj123v，arm64/macOS 27.0.1。安装路径 ~/Applications/DSH Client.app，不注册自启、不动 DSH worker。此配置用于分发；客户端 SSH 转发配置另由用户选择。

## 分发

在构建机完成测试和 --bundles app --no-sign 后，用 ditto 生成 ZIP，记录 SHA-256。scp 到 .13 临时目录，远端核对 ZIP 与 Mach-O 二进制哈希和架构。更新前关闭 **客户端**，保留旧 APP；不要 restart/kickstart DSH。

```bash
scp -F ~/.ssh/dsh-fleet-config <verified-zip> dsh-air:/tmp/DSH-Client-arm64.zip
ssh -F ~/.ssh/dsh-fleet-config dsh-air 'shasum -a 256 /tmp/DSH-Client-arm64.zip'
```

解压到独立临时目录，用 ditto 复制 APP 到用户 Applications，open 启动。未签名测试包如触发 Gatekeeper，在 Finder 右键打开，仅对本次受信任产物操作。

## 用户测试步骤

1. 新建 HTTPS/Web 连接：名称“.13 本机”，地址 127.0.0.1:3080；保存。
2. 在左侧这条连接的一次性 token 框粘贴当前 worker 的 token **字符串**，点“打开 / 重试”。认证失效后重复此步，不把 ?token= URL 保存为地址。
3. 查看工作区/会话和模型选择，发一条最小消息，确认回复持续更新。关闭连接窗口再空 token 打开，确认 Cookie 恢复。
4. 新建 SSH 转发：选择正确 SSH 配置、Host 别名、用户与远端端口。先用终端确认该 Host 可非交互公钥登录且已确认指纹。dsh-service 受限 key 若不允许转发，需要普通用户转发权限。
5. 窗口关闭/客户端退出后核对所属 ssh 子进程已消失；不以 HTTP 401 判为业务通过。
6. 编辑保存会关闭旧窗口；删除会请求系统异步清理浏览数据，严格清理完成仍需独立测试。

## 回滚

关闭客户端，保留失败 APP 供检查，恢复安装前留存的旧 APP 后 open。只删除经过核实的客户端路径；不删除 .dsh、worker Profile、凭据或会话。具体安装版本、哈希、PID 和结果见 DSH 主仓验收记录。
