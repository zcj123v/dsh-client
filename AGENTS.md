# DSH Client 维护规范

- 纯客户端边界：不得引入 DSH Host/Node 执行体或修改节点监听、Profile、执行配置。
- 默认文档中文；新能力必须写明实际支持平台，架构目标不能写成实现完成。
- URL/认证/SSH/IPC/分区改动必须补真实行为回归测试；凭据不得进入测试真实值、日志、Git 或连接配置。
- 原生安全必须在 Rust 独立校验，不能只依赖前端。
- 连接窗口关闭、失败和退出均需回收已启动及 pending SSH；禁止任意命令接口。
- 验证：pnpm test、pnpm typecheck、pnpm build、cargo test --manifest-path src-tauri/Cargo.toml --lib；平台 APP 需真实运行验收。
- 发布未签名测试包必须说明版本/哈希/架构；不把 HTTP 401 当作业务健康，不把编译通过当作登录/流式通过。
- 用户级安装可回滚，不注册自启，不重启目标 DSH。
