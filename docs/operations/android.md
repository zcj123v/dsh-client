# Android 原生内容容器

## 当前范围

Tauri/React 本地连接列表 + 单个独立 Android 内容 Activity，目标 DSH 提供全部业务界面。Android 只支持已有 HTTPS 入口；不运行 DSH、不调用系统 SSH、不为客户端扩大节点监听。

原生工程已生成并成功编译 arm64 debug APK（SDK/target 36，minSdk 24，NDK 29.0.14206865、JDK 21）。APK 编译不替代设备认证/会话/隔离验收。2026-11-16 模拟器 API36 / System WebView 133.0.6943.137 已通过3项仪器测试：Cookie/LocalStorage隔离和恢复、Activity重建、清单权限。真实DSH业务和个人真机仍待测试。

## 存储与权限

- 本地 Tauri main WebView 使用默认存储，远端内容使用普通 Android WebView，完全没有 Tauri/JavaScript 原生桥。
- 每次创建远端 WebView 后、配置和导航前，调用 AndroidX `WebViewCompat.setProfile(view, "dsh-<connectionUUID>")`。Cookie/WebStorage 由命名 Profile 隔离。
- 先检查 `WebViewFeature.MULTI_PROFILE`。不支持时拒绝打开并要求更新系统 WebView，不降级为共享存储。不能仅根据 Android API 版本判断此特性。
- token 通过本地壳→Rust→进程内存交给内容页；Intent 只携带连接 ID，认证信息不写入 Intent/Bundle/连接配置。Activity 重建只恢复无 query 的根地址/id/name，以同分区 Cookie 重开；不保存 WebView 历史。导航完成后移除当前地址查询参数。
- Activity 不导出；关闭应用备份；内容窗口设 FLAG_SECURE，防止任务快照/录屏收集 token 页面。文件上传只通过系统 Open Document 选择器的临时 content URI；不申请全盘文件权限。
- 主导航只允许连接 origin，用户手势外链才交系统浏览器；外链拒绝凭据参数、片段和非 HTTPS scheme。TLS 错误始终取消，不提供绕过按钮。混合内容、地理位置、媒体权限拒绝。
- 当前测试版明确不支持原生下载；复杂 popup/OAuth、长时间后台保持等需要继续兼容验证。

## 删除分区

AndroidX 不允许删除当前进程已加载的 Profile，即使其 WebView 已销毁。删除连接先持久记录 UUID 墓碑、关闭内容页和移除连接；下次主进程启动、加载任何连接 Profile 前尝试 `ProfileStore.deleteProfile`，成功/不存在则移除墓碑，失败保留待下次重试。系统磁盘删除仍可能异步，不承诺同步物理擦除。

## 构建

```bash
pnpm install --frozen-lockfile
source "$HOME/.cargo/env"
export ANDROID_HOME=<SDK根目录>
export NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
export JAVA_HOME=<JDK21目录>
sdkmanager 'platforms;android-36' 'build-tools;36.0.0' 'ndk;29.0.14206865'
rustup target add aarch64-linux-android
pnpm test
pnpm typecheck
javac -d /tmp/dsh-client-android-policy-tests tests/AndroidPolicyTest.java src-tauri/gen/android/app/src/main/java/com/zcj/dshclient/ContentPolicy.java
java -cp /tmp/dsh-client-android-policy-tests AndroidPolicyTest
pnpm tauri android build --debug --target aarch64 --apk
```

产物位于 `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`，仅包含 arm64-v8a。debug 签名用于个人测试，正式分发需要独立发布签名配置，不提交 keystore。Tauri CLI 会为当前机器生成忽略的 `tauri.settings.gradle`、Wry Kotlin glue 和链接的 native library；fresh checkout 必须先走 CLI 构建，再运行 Gradle 测试，不提交绝对路径。

## 设备验收

```bash
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
adb shell am start -n com.zcj.dshclient/.MainActivity
# 构建完成后，复用本轮 native library 编译测试 APK
cd src-tauri/gen/android
./gradlew :app:assembleUniversalDebugAndroidTest -x rustBuildUniversalDebug
adb install -r app/build/outputs/apk/androidTest/universal/debug/app-universal-debug-androidTest.apk
adb shell am instrument -w com.zcj.dshclient.test/androidx.test.runner.AndroidJUnitRunner
```

自动仪器测试使用虚拟 HTTPS origin 与假值，检查 Cookie/LocalStorage 隔离、销毁后同分区恢复、Activity 重建及清单权限。实际 DSH 认证/对话、网关 Basic Auth、TLS 失败、系统附件选择、网络恢复还需在目标设备执行；只看到 401 不算通过。

## 官方依据

- [AndroidX WebView Profiles](https://developer.android.com/reference/androidx/webkit/Profile)：Profile 数据互不共享。
- [WebViewCompat.setProfile](https://developer.android.com/reference/androidx/webkit/WebViewCompat#setProfile(android.webkit.WebView,java.lang.String))：必须在配置/导航前设置。
- [ProfileStore.deleteProfile](https://developer.android.com/reference/androidx/webkit/ProfileStore)：已加载 Profile 不可删除，磁盘删除可能异步。

## 2026-11-16 平台验收证据

本机专用模拟器 emulator-5580：arm64/API36，System WebView 133.0.6943.137。3项仪器测试初跑中 Activity 重建红（重建后视图0），修复后全部通过；前端13测试和Rust7测试通过，Java URL边界16检查通过。

从实际 Tauri 主界面启动（http://tauri.localhost），标题和连接列表正确、SSH入口隐藏；通过原生 open_connection 命令打开私有 DshContentActivity 并成功加载 https://example.com/；DevTools 检查远端 `typeof window.__TAURI_INTERNALS__` 为 undefined。调用删除后仅UUID墓碑持久化，force-stop/restart后墓碑集合为空。未将公共页面加载替代DSH认证/会话业务验收。


## 0.2.0 构建固化

源码 `ec91d3c`，Android [CI43815786483](https://github.com/zcj123v/dsh-client/actions/runs/43815786483) 与三桌面 [CI43815786494](https://github.com/zcj123v/dsh-client/actions/runs/43815786494) 全成功。实际本机构建APK versionCode2000/versionName0.2.0/minSdk24/target36/arm64-v8a，SHA-256 `7a08aa1c389de89017c800830f7be8f5c675629cfa414c7cf502f5b25033c744`；3项仪器测试包含恢复Bundle无假token的断言，最终重跑通过。

Actions artifact 只保留14天；debug签名依赖构建机，Android同包名不同debug签名不能覆盖安装。换机器产物时先确认签名；卸载会删除客户端连接与登录数据，必须由测试者自行决定后操作。CI下载物与本机APK不保证字节相同，均按各自产物SHA256核对。
