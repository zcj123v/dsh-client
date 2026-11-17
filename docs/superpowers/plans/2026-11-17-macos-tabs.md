# macOS Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use executing-plans; UI may delegate one disjoint implementation scope, final whole-branch review required.

**Goal:** 用户批准的同窗口浏览器式多标签、固定管理页、Dock恢复，macOS0.4预览与.13更新。
**Architecture:** Rust macOS child WebView替代独立remote Window，固定56logical顶部壳栏；native tab registry保存opening/active/closed和精确generation。React壳仅管理bar/chooser，切换showhide不导航。其他平台旧路径不构建。
**Tech Stack:** Tauri2.12.5 unstable desktop child APIs、WKWebView隔离、React19/TS6/Rust。
**Spec:** docs/superpowers/specs/2026-11-17-macos-tabs-design.md

## Global Constraints
- 纯客户端，无Host改动/重启；先仅macOS，授权通过后本轮升级.13并备份0.3。
- IPC检验actualWebview main+localorigin；capabilities仅webviews main；远端child无原生权限。
- 固定顶部56logical，resize/scale显式set_bounds，不auto_resize比例定位；管理/+隐藏所有remote视图。
- 在probe/await前register owned session；closebeforebuild/afterbuild/token/错误/退出精确代际取消回收；切换不导航。

## Review Focus
- remotechild与main同window不得继承IPC，须真实拒绝回归/实际探针。
- 首次打开/重试中关标签、编辑/删节点、退出都无SSH残留，旧attempt不能改新标签。
- resize/Retina后bar始终可点击，隐藏child亦更新；管理/plus不被nativeoverlay遮住。
- 主窗红按钮hide/Dock Reopen恢复已有views；CmdQ才断开全部；CmdW关active页签。
- state同步失败时主壳可回management重取tab registry，不靠前端假state认为已连。

### Task1: 原生标签、权限和窗口生命周期（lead scope src-tauri/**）
**Interfaces:** list_tabs()->{tabs:[{id,name,state,generation}],activeId:string|null}; switch_tab(id:string|null)->same snapshot; close_tab(id:string)->same; open_node retains {generation,endpointIndex}, macOS creates child/activates; event tabs-changed payload snapshot; menu event tab-action payload {action:'manager'|'new'|'close'}.
- [ ] 写native TabState纯状态行为测试duplicate/close-before-open/activecancel/generation，cargo test观察红。
- [ ] 新src-tauri/src/tabs.rs owned registry with Cancelled bool、snapshot、layout；sources lookupWebview/macOS与非macOS cfg保留；Rustcommand签名actualWebview。
- [ ] 真实sleep/HTTP回归取消；require_shell_origin(label,url)纯函数labelmain/root校验；capswebviewsmain。
- [ ] 菜单Cmd1/T/W、CloseRequested hide、Reopen show；Resized/scale更新所有child。
- [ ] cargo test全通过，rustfmt，commit。

### Task2: 标签UI（disjoint delegate src/**,tests/**）
**Interfaces:** Task1 above；detect navigator.userAgent MacOS only then native tabs enabled; Windows/Linux/Android currentUI kept. IPC APIs exactsnake_case,JSargs camelCase (id nullable)。
- [ ] 先写App标签/＋existingnew/managerfixed/switchdoesnotopen/closeadjacent/reloadsnapshot tests并观察红。
- [ ] 顶部56CSSpx固定bar，horizontaloverflow；plus openschooseronmanagerhidechild；list_tabs initial/read event。close usesnative结果；open_node后list native eventsupdate。existingtab点击只switch，不重放token（明确用户输入token reopen例外）。
- [ ] Cmdmenu eventtab-action drivesmanager/new/close via samehandlers；固定管理页可始终返回；原nodepriority回归保留。
- [ ] pnpmtest/typecheck/build，commit UIscoped。

### Task3: 集成、审查、实际APP与.13
- [ ] 集成双方diff/运行TS+Rust，freshreadonlygptreview，修Critical/Important及相关回归。
- [ ] 版本0.4.0仅arm64macOS构建，nativechild真实双view切换保留输入、plus/manage/resize/Dock复现/close进程回收/actualDSHCookie。
- [ ] ZIPsha、安装.13备份0.3，准确机器/workerPID不变，记录启动与未验范围；交付用户测试。

## Review/Execution
用户已批准设计并授权两功能，沿用上一轮native实施+finalreadonlyreview；不再请求重复流程确认。方案采用A，macOSnativeB不足以自定义+chooser，iframe因CSP与分区淘汰。
