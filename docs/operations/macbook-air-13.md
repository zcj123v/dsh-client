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

1. 新建 HTTPS/Web 连接：名称“.13 本机”，地址 `http://127.0.0.1:3080/`（必须包含协议）；保存。
2. 在左侧这条连接的一次性 token 框粘贴当前 worker 的 token **字符串**，点“打开 / 重试”。认证失效后重复此步，不把 ?token= URL 保存为地址。
3. 查看工作区/会话和模型选择，发一条最小消息，确认回复持续更新。关闭连接窗口再空 token 打开，确认 Cookie 恢复。
4. 新建 SSH 转发：选择正确 SSH 配置、Host 别名、用户与远端端口。先用终端确认该 Host 可非交互公钥登录且已确认指纹。dsh-service 受限 key 若不允许转发，需要普通用户转发权限。
5. 窗口关闭/客户端退出后核对所属 ssh 子进程已消失；不以 HTTP 401 判为业务通过。
6. 编辑保存会关闭旧窗口；删除会请求系统异步清理浏览数据，严格清理完成仍需独立测试。

## 回滚

关闭客户端，保留失败 APP 供检查，恢复安装前留存的旧 APP 后 open。只删除经过核实的客户端路径；不删除 .dsh、worker Profile、凭据或会话。具体安装版本、哈希、PID 和结果见 DSH 主仓验收记录。

## 2026-11-17 升级到 0.3.0 macOS 预览版

用户明确要求升级`.13`后，核对主机 `zcj123vdeMacBook-Air.local`、arm64、原安装0.1.1。传输已验证的0.3.0 ZIP，远端SHA256与本机一致：`ce74b92ef06105cb6e152a102b526dd5e77e30c7a87b5fc9ad17f400387eb9d0`。原客户端当时未运行，无需强制终止。

原APP完整保留在 `~/Applications/DSH Client-0.1.1-backup-20261117.app`，新APP安装到 `~/Applications/DSH Client.app`。安装后Plist版本0.3.0，Mach-O SHA256 `f19e0f4e3522adf1cbd42b1d2027206de9cdf65024b364486ed5c5f3bdc62a8d` 与包内一致，open启动后核对精确可执行路径，客户端PID83429。更新前后DSH监听 `127.0.0.1:3080` 均为原PID10813，未重启Host、未注册自启、未改节点连接或认证数据。

本次证明安装与进程启动，未远程验证该机窗口画面、登录或LAN/WG实际业务；用户可在0.3.0的“添加本机 DSH”和节点入口列表体验。包仍为macOS14+ arm64未签名预览；其他平台未打包。需要回滚时关闭0.3.0，保留它供排查，再恢复上述0.1.1备份。
