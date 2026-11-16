# Node Priority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task. Steps use checkbox syntax.

**Goal:** 本机快捷添加、节点多入口固定顺序回退，只交付macOS。
**Architecture:** 旧ConnectionRecord保留兼容字段，新增可选endpoints数组；Rust open_node依序调用现有open_connection生命周期，成功返回generation和endpointIndex。UI存节点/入口，不存凭据或选中状态。
**Tech Stack:** React/TypeScript/Tauri/Rust，reqwest native-tls系统信任。
**Spec:** docs/superpowers/specs/2026-11-16-node-priority-design.md

## Global Constraints
- 1–8入口，direct探测3秒，无token/redirect/body读取；SSH5秒首次就绪，固定参数和严格known_hosts。
- 只构建macOS，其他平台用户确认体验后再打包，不触发其他端CI；不更新.13或Host。
- 原节点UUID/partition/SSH本地origin保持；活动窗口不自动切换。Android保持原单入口。

## Review Focus
- 恶意第二入口不得绕过Rust URL/SSH校验；全部失败token不发送。
- 第一入口401是可达，不能误试下个；失败SSH不得留子进程。
- 编辑/关闭后无旧回调污染新窗口；打开活动节点保持当前入口。
- 旧连接/多入口顺序和脱敏必须在保存重载后保持，打开时间不能改变列表顺序。
- 本机快捷重复点击不重复；LAN/WG不同origin不拷贝Cookie，表单上下调整与重开行为一致。

### Task 1: 节点数据与管理UI
**Files:** src/connection-model.ts,src/connection-store.ts,src/App.tsx,src/App.css,tests/connection-model.test.ts,tests/connection-store.test.ts,tests/App.test.tsx
**Interfaces:** Endpoint包含label/mode/url/sshConfig/sshProfile/sshUser/remotePort；ConnectionDraft/Record可含endpoints。旧top-level字段表示第一入口以兼容旧端。
- [x] 先写多入口保存重载顺序/拒非法URL/secret白名单、本机按钮去重和入口移动测试，pnpm test验证红。
- [x] 实现1–8端点校验与深拷贝、固定节点顺序、本机快捷及增删/上下移表单。
- [x] pnpm test/typecheck/build通过，记录结果。

### Task 2: 原生有序探测/打开
**Files:** src-tauri/src/lib.rs,src-tauri/src/endpoints.rs,src-tauri/Cargo.toml,src-tauri/Cargo.lock
**Interfaces:** open_node(window,app,record,token)->{generation:u64,endpointIndex:usize}；复用open_connection单入口命令并保留来源检查。
- [x] 写真实本地HTTP探针超时/401/500/重定向不跟随/按顺序成功停止测试及原生整表校验，cargo test观察红。
- [x] 实现native probe和入口顺序循环，清理失败SSH，已有窗口使用存储active index；状态仅发本地shell，token仅成功入口打开。
- [x] Rust全套、TS全套/typecheck/build通过。

### Task 3: macOS测试包与真实窗口验收
**Files:** package.json,src-tauri/Cargo.toml,src-tauri/tauri.conf.json,docs/operations/local-macos.md
- [x] 独立只读整体审查并修阻断问题，记录边界。
- [x] 版本0.3.0仅本机macOSarm64构建，ZIP/sha，实际运行新壳测试本机快捷/入口保存和第一不可达第二可达/活动窗口保持/关闭重开。
- [x] 更新操作说明并交付macOS包，保持独立分支供体验迭代，不触发其他平台。
