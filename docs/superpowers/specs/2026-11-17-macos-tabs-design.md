# macOS 同窗口多标签与节点管理恢复设计

2026-11-17；承接用户明确提出：标题栏处类似浏览器的多标签、＋选择已有或新增节点；关闭编辑/导航页后必须能回来。只先打macOS包，保留LAN→WG优先入口及纯客户端边界。

## 体验

一个原生窗口，窗口标题下固定56逻辑像素标签栏：`[节点管理] [本机 DSH ×] [k8s ×] [+]`。节点管理固定、不可关闭；每节点只一个标签，重复选中切回原标签，不新开重复连接。切换标签隐藏内容但保留页面、输入和SSH，不重新导航。关闭节点标签关闭该WebView并精确回收对应SSH，不删除节点或Cookie。关闭活动标签返回相邻标签，无节点时显示管理页。

＋打开本地节点选择面，列出已有节点和“新增节点”；选择已有节点使用其LAN/WG顺序打开，已有标签直接切换。新增进入管理表单；管理页上的原“打开/重试”改为打开标签/切换已有标签；带一次性token时仍允许明确重新认证。界面显示加载/尝试入口和错误，不把401当已登录。

关闭红色窗口按钮隐藏主窗口并保留标签，不销毁管理页；点击Dock图标重新显示。系统菜单增加“节点管理”(Cmd+1)、“新建连接标签”(Cmd+T)、“关闭当前标签”(Cmd+W)；Cmd+Q才真正退出并回收全部SSH。管理入口永远可见，与远端DSH页面不共用导航。

## 方案取舍

macOS系统原生窗口合并标签可保留现有独立窗口，但其＋默认创建窗口而非选已保存节点，管理页仍是独立可销毁窗口，无法稳定满足统一导航。iframe会受目标CSP/X-Frame-Options阻挡且认证/分区控制不足。采用Tauri原生child WebView，同一个主窗口里独立内容视图，标签栏由本地壳提供，不使用iframe。

## 原生与安全

仅macOS启用Tauri unstable child WebView API；其他平台本轮不构建/发布且保留独立窗口实现。远端view仍按节点UUID独立WKWebsiteDataStore；LAN/WG SSH沿用节点固定本地端口。保留来源导航白名单、外链系统浏览器、token瞬态输入、TLS和原生全入口校验、有限重连。

child位于主壳栏下方56逻辑像素、尺寸随窗口resize/scale更新；切换show/hide/set_focus，不eval远端页面管理标签。主壳保留整个窗口大小，child覆盖内容区；显示管理/＋时隐藏所有远端child，避免原生叠层遮住选择面。

IPC来源检查必须接受实际发起调用的tauri::Webview并验证webview.label()==main、URL为本地壳；不能用共享window.label()==main判断。capability仅给webviews:[main]，不能以windows:[main]授权所有child。新标签控制命令只允许本地壳。进程创建/尝试/恢复继续精确Arc/代际约束；关标签取消pending，错误/退出kill+wait。child没有独立Destroyed窗口回调，改用本地拥有的TabSession取消域，关闭/编辑/删节点/退出统一触发，不能把每个child挂主Window::Destroyed当独立关闭。

管理状态由Rust owned tab registry维护并提供list/switch/close；前端展示状态并处理菜单事件。原生tab存在即只切换而不重新probe或navigate（token明确非空重认证除外）；刷新本地壳后可重新读取registry。关闭窗口隐藏所有视图由OS一起完成；Reopen显示主Window并恢复activeview。真正退出停止全部pending/active进程。

## 验收

前端：＋已有/新增、去重、管理不可关、标签切换不调用open、关闭相邻选择、错误可回管理。Rust：未授权remotechild调用被拒、tab取消在pending前/后都回收、关闭标签不影响其他节点、selected entry/恢复状态代际保持。保留所有旧测试。

真实macOSAPP：两实际child同时存在、同窗口标签bar常驻、切换保留文本与页面标记、＋选择管理、关闭/重新打开、缩放/resize边界、红按钮隐藏后Dock重现、Cmd+Q进程回收、实际DSH Cookie/页面载入。构建版本0.4.0 arm64测试ZIP并验证SHA；通过后按既有授权升级.13，备份0.3.0，不改Host监听/服务或凭据。

## 状态

用户已通过交互确认选择“按此设计实现（推荐）”，包括红按钮隐藏/Dock恢复、CmdQ退出。原生API可行性只读复核进行中；采用实现中明确的API边界修订，不扩大交互范围。
