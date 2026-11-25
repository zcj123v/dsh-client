# DSH Client 节点 JSON 导入导出设计

**状态：设计已确认。**

## 意图

让 `https://dsh-token.zcj123v.online/` 与 DSH Client 共享一种可读、可手工编辑的节点 JSON。网站可按入口类型下载或复制节点配置。客户端支持文件导入、粘贴导入、直接编辑 JSON，以及完整备份所有已保存的连接，包括本机 DSH 和自定义入口。备份覆盖连接名称、入口参数和优先级。唯一可选的凭据是 DSH 首次登录 token。

## 现状与约束

- 客户端已支持每节点 1–8 个有序入口、桌面 SSH local-forward 与 HTTPS、Android HTTPS-only、跨入口有序回退；HTTP 远程入口被拒绝。
- 节点连接记录存 `localStorage`，Token 只在 UI 内存态输入，未持久化。连接 partition 由本机 UUID 派生。
- Token 站点已有 `data.json`：节点的 `name/ip/url/lan/token`。其中 `lan` 是 DSH Web HTTP/LB 地址且可能为空/不可用，**不是 SSH 目标**；裸机 `lan` 现为空，K8s 的 `172.18.18.25:3081` 是 HTTP VIP。站点有 Basic Auth 门禁，不能把它扩展为分发其它密码。
- Windows/Linux 仍是每节点独立窗口；macOS 有原生多标签；Android 不支持 SSH。本功能不改变这些平台能力。

## 方案比较

1. **共享可读 JSON（推荐）**：网站与客户端使用同一版 schema；支持文件和剪贴板，以及客户端 JSON 编辑。与手工编辑、离线迁移一致，不新增后台协议。
2. **网站直接推送客户端**：需客户端监听服务、配对码或自定义 URL 协议，增加攻击面、服务生命周期和部署工作，不适合当前静态 Token 页面。
3. **不同的站点与客户端配置格式**：初期简单，但入口、token、更新行为容易漂移，不能可靠往返编辑。

选择方案 1。传输 JSON 是明文；加密打包不在首版范围，因为用户要求可直接编辑。

## 网站导出

在 Token 页面新增“导出客户端 JSON”，入口类型是三个独立多选项：

```text
☑ LAN / VIP + SSH       默认勾选
☑ WG IP + SSH           默认勾选
☐ WG 域名 + HTTPS       默认不勾选
☐ 包含 DSH 登录 token   默认不勾选
```

所选入口按 `LAN/VIP SSH → WG IP SSH → WG 域名 HTTPS` 顺序写出；用户可在客户端 JSON 编辑器中调整实际优先级。每节点最多只写出已核实且明确启用的选项；某节点缺少对应地址时保留该节点的有效其它入口并列出遗漏原因，无可导出入口时在预览列出并跳过，不合成猜测地址。

- 新增受控出口元数据：`lanSshHost`、`sshUser`；由站点维护者按节点逐个填写、验证。不得复用 `nodes[].lan` 作为 SSH Host。
- WG SSH Host 取受控节点清单的 `ip`（例如 `10.250.0.x`），SSH 用户取受控映射的用户。站点提供 Host/User，不传 SSH 密钥或口令。导出前只生成 JSON，不探测 SSH 私钥或其可用性。
- WG HTTPS 取 `nodes[].url`，必须是 HTTPS 根地址。该域名是现有 DSH Web 入口，不构造 WG HTTP IP 直连。
- token 仅当显式勾选时从相同节点 `nodes[].token` 写出；空 token 不写或为 null，不从其他服务凭据补齐。勾选 token 后下载/复制前显示明文敏感数据警告及确认。
- 提供“下载 JSON”和“复制 JSON”。下载文件不含静态站点 `creds`；Clipboard 只放用户选择的 client config。

## 共享 JSON 格式 v1

```json
{
  "format": "dsh-client-config",
  "version": 1,
  "nodes": [
    {
      "key": "macbook-air",
      "name": "MacBook Air",
      "endpoints": [
        {
          "kind": "lan-ssh",
          "label": "LAN / VIP + SSH",
          "mode": "ssh-forward",
          "sshConfig": "default",
          "sshProfile": "192.168.1.20",
          "sshUser": "user",
          "remotePort": 3080
        },
        {
          "kind": "wg-ssh",
          "label": "WG IP + SSH",
          "mode": "ssh-forward",
          "sshConfig": "default",
          "sshProfile": "10.250.0.13",
          "sshUser": "user",
          "remotePort": 3080
        },
        {
          "kind": "wg-https",
          "label": "WG 域名 + HTTPS",
          "mode": "direct",
          "url": "https://dsh-air.example/"
        }
      ],
      "token": "example-token"
    },
    {
      "key": "local:2d1fc21e-40d0-4eb4-a611-e39dbbbcc52e",
      "name": "本机 DSH",
      "endpoints": [
        {
          "label": "本机",
          "mode": "direct",
          "url": "http://127.0.0.1:3080/"
        }
      ]
    }
  ]
}
```

- `token` 可缺省，示例为假值。解析器按层级校验字段白名单，拒绝未知字段，包括 `password`、`secret`、`creds` 和 SSH 密钥字段。错误只标出位置和字段名，不回显凭据值。
- 入口数组顺序就是回退顺序。网站为三类入口写入 `kind`。客户端的本机入口和自定义入口可省略 `kind`，按现有 `mode` 和参数完整往返。已声明的 `kind` 必须与连接方式一致。
- `sshConfig` 只能是现有的 `default|fleet|none`。`sshProfile` 保存系统 SSH Host 别名或 IP，`sshUser` 是可选登录名，`remotePort` 默认 3080。备份引用这些 SSH 设置，恢复设备仍需具备相应的 SSH 配置和信任记录。
- 桌面完整备份保留 `http://127.0.0.1:3080/` 等现有允许的回环入口。恢复后，“本机”指运行客户端的设备。远程直连继续要求 HTTPS。

## 客户端体验与合并

1. 节点管理区新增 JSON 导入、导出和编辑入口，并提供「从 Token 网站获取」链接。链接在系统浏览器打开现有 Token 页，由用户完成网站认证并下载或复制 JSON。客户端可以选择 `.json` 文件或粘贴文本。JSON 编辑器展示完整可读配置，语法错误标出行列，校验错误标出节点和字段。
2. 先展示预览：新增节点、将更新的节点、保留原有本机节点、缺失/平台不支持入口、是否携带 token。只有用户确认后原子写入；任何整体 schema/校验错误不部分导入、不覆盖现有数据。
3. 导入只更新文件中声明的节点，其余本机节点原样保留。`key` 是用于匹配的备份标识。既有记录首次导出时为其分配稳定标识，网站节点使用维护清单的标识。重复导入同一 `key` 时更新名称和入口。入口目标集合不变时保留本机 UUID、分区及登录态。入口目标变更时保留备份标识与时间戳，使用新的本机 UUID 和浏览分区，保存后请求清理旧分区。新设备恢复时为新记录分配本机 UUID 和分区。旧连接可手动绑定网站 `key`。备份范围限于连接配置和用户选择的当前内存 token，浏览 Cookie 及会话数据留在原设备。
4. 导入 token 后填入当前运行期间的节点登录输入状态。用户点击连接时沿用现有首次登录流程，token 只在选定入口的登录请求中使用。连接探测、普通连接配置和日志均不携带 token。客户端导出默认为完整配置备份，可另行勾选带上当前内存中的 token。客户端退出后，已有 Cookie 仍按现有机制恢复登录。需要再次认证时可重新导入 token 或手工粘贴。
5. JSON 编辑器和文件/剪贴板导出均应防止意外明文带出：含 token 时明显标识，复制/下载前确认；不提供自动写剪贴板、不上传回网站、不在埋点/错误提示回显 token。
6. 桌面导入后保留入口优先级并依次连接。Android 用同一文件 schema，但导入预览标出 SSH 入口不可用，跳过 SSH，仅按顺序尝试 HTTPS；若没有 HTTPS 入口，阻止打开并解释需要新增 HTTPS，而不是静默改用 HTTP。

## 安全、非目标

- 不实现 token 的加密持久化、云端同步、自动下载/更新或客户端到网站上传。
- 不传输/储存 SSH 私钥、passphrase、网关 Basic Auth，也不从 Token 页 `creds` 节点导出其它服务口令。
- 不更改节点 DSH 监听、服务、Profile、Token 采集机制或域名代理。
- Website 导出前验证只依据受控元数据的完整、格式和已记可用状态；SSH 连通性由客户端现有系统 OpenSSH/known_hosts 规则验证，导入/导出不自动接受新主机指纹。
- 站点 HTTP Basic Auth、浏览器下载目录、剪贴板和明文 JSON 都是敏感边界；UI 明示责任，用户自行通过安全渠道传递/删除文件。

## 验收要点

- 用含 LAN SSH + WG SSH + HTTPS + 假 token 的 fixture 做网站三选项组合导出，顺序、缺字段提示、无凭据 JSON 与 token JSON 字节/语义正确；输出绝不含 `creds`、私钥、Basic Auth。
- 客户端 v1 JSON parser 先测合法/缺字段/错误 scheme/注入 SSH 参数/未知凭据字段/超限入口/畸形 token；在 Tauri IPC/Rust 层重复关键安全校验。
- 重复导入更新稳定 `key` 时保留未导入节点，入口目标不变时保留原 UUID 和分区，目标有变化时使用新分区。客户端完整备份包含「本机 DSH」、自定义 HTTPS、SSH 别名及入口顺序，导入同一设备或空白设备后均能恢复这些设置。失败导入无写入。重启后客户端不保留 token，默认导出无 token，确认后可导出内存中的 token。
- 桌面覆盖顺序回退、host-key 首连限制不变；Android 对只有 SSH 的节点给明确不可用提示，对含 HTTPS 节点跳过 SSH 使用 HTTPS。
- 站点构建/前端测试与客户端 `pnpm test`、`pnpm typecheck`、`pnpm build`、Rust lib tests；最后逐平台构建，Android APK 不替代个人设备业务验收。

## Token 生命周期

本设计沿用当前的内存机制。导入的 token 在当前运行期间可用于登录和再次导出，退出后清空。换设备时可导入原来的含 token JSON，或从网站重新获取。本版本不跨重启保留 token。
