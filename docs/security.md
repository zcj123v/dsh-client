# 安全边界与已知限制

- 客户端不运行 DSH Host，不改任何节点监听、Profile、LaunchAgent 或执行配置。
- 远程 direct 仅 HTTPS；HTTP 只允许 loopback。节点默认 loopback，通过现有 HTTPS 网关或 SSH 进入。
- URL 禁止 userinfo、查询参数、片段和子路径；token 必须用独立瞬态输入。配置按白名单持久化并校验恢复数据。
- 原生 command 接收连接字段，不接受任意进程参数。前端和 Rust 两层校验，来源必须为本地 main。
- SSH 使用系统配置/密钥/known_hosts，BatchMode=yes、StrictHostKeyChecking=yes；客户端不读取私钥、不自动接受未知主机指纹。
- per-connection WebView 与 origin 导航守卫隔离远端页面；远端不获得 Tauri IPC。
- token 加载完即从当前导航历史项移除。WebView 内部瞬态网络请求/缓存由平台处理；不承诺可证明的内存擦除或对系统诊断隐藏。
- 删除连接请求异步浏览数据清理，系统 API 不提供完成回调；UI 明确为“请求清理”，旧分区 ID 不复用。严格同步擦除仍需平台实现与独立验收。
- 外链过滤不是远端页面的完整数据防泄露沙箱；只有受信任的个人 DSH 入口应加入连接列表。
- macOS 分区最低要求系统 14；Android 以独立命名 Profile 隔离，不支持特性时拒绝打开；Windows/Linux 已原生 CI 构建，用户桌面仍待验证。
- TLS 仅使用系统信任链，不静默忽略证书错误；证书指纹 pinning、自签名例外、Android SSH 都尚未实现。
- 运行态不得打印 token、Cookie、Authorization、Basic Auth 或私钥。单元测试只使用假值。

- Android 远端容器不是 TauriActivity，无 JavaScript interface。Activity 不导出、备份关闭、token 不进入 Intent/恢复 Bundle；HTTPS-only、混合内容拒绝，内容页截屏受 FLAG_SECURE 保护。已加载的 Profile 删除使用持久墓碑，下次进程启动时处理，不能承诺本次调用已完成磁盘擦除。

- Android0.2.2保持主导航认证/网络/TLS错误直到下一次导航，不由onPageFinished覆盖。真实自签名loopbackTLS探针已验证系统证书拒绝、失败提示保留且页面不执行；未导入测试证书，不放宽信任链。系统可信证书/SAN路径属于当前支持范围，可选pinning及自签名例外仍不支持。
