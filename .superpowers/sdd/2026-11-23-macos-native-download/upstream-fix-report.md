# rc3 Desktop HTTP(S) 原生下载旁路修复报告

日期：2026-11-24

## 结论

已在补丁交付目录中完成根因修复，未修改实际上游安装文件、`src/file-actions.ts` 或 `tests/file-actions.test.ts`。Desktop 存在且原始 URL 为绝对 HTTP(S) 时，`BrowserFileActions.save` 现在跳过 `source.read` RPC 和 Blob URL 转换，把原始 URL 直接交给 `desktop.save`。Browser 路径及 Desktop 非 HTTP(S) 的 Blob 兼容路径保持不变。

补丁基线为上游 tag `dsh-v0.2.2-rc.3`，commit `38a6a5c965aa99ab525161efb7cc5cf1fe62fced`。最终差异为 2 个文件、18 行新增、2 行删除；已验证可干净应用到该基线。

## 桥接契约与根因

- `src-tauri/src/downloads.rs` 的 `DownloadRequest` 只接收 `url/name/mediaType/mode`，`save_download` 对原始同源 HTTP(S) URL 做独立校验、Cookie 获取及 Range 流式保存。
- `src/file-actions.ts` 只接受 HTTP(S) URL，并通过 `invoke('save_download', { request: clean })` 调用原生保存；它不接受 `blob:` URL。
- rc3 上游 `BrowserFileActions.save` 的 Desktop 分支原先只要 `source.read` 存在就执行完整读取，再调用 `URL.createObjectURL(blob)`。因此 Desktop 菜单保存会在进入原生桥接前失败；大于 32 MiB 时更早在 RPC 读取阶段失败。

## 补丁文件与应用位置

| 补丁交付文件 | 上游目标 |
| --- | --- |
| `patches/dsh-client-ui-file-actions-0.2.2-rc.3-download-stream/files.ts` | `packages/client/ui-file-actions/src/client/files.ts` |
| `patches/dsh-client-ui-file-actions-0.2.2-rc.3-download-stream/files.client.testsource` | `packages/client/ui-file-actions/tests/files.client.spec.ts`（应用时恢复上游文件名） |

实现仅在 Desktop 分支增加 HTTP(S) 判定：

```ts
const direct = URL.canParse(source.url) && ['http:', 'https:'].includes(new URL(source.url).protocol)
const blob = source.read === undefined || direct ? undefined : await source.read(this.lifetime.signal)
```

`blob` 因此在该分支保持 `undefined`，后续原有代码自然选择 `source.url`。未增加新接口、依赖或配置。

应用验证使用临时 rc3 clone 生成 diff，恢复 pristine 后执行：

```text
git apply --check /tmp/dsh-rc3-upstream.BL5m45/native-download.patch
git apply /tmp/dsh-rc3-upstream.BL5m45/native-download.patch
cmp <应用后的 files.ts> <交付 files.ts>
cmp <应用后的 files.client.spec.ts> <交付 files.client.testsource>
```

全部退出 0；`git diff --stat`：

```text
 packages/client/ui-file-actions/src/client/files.ts     |  3 ++-
 .../client/ui-file-actions/tests/files.client.spec.ts   | 17 ++++++++++++++++-
 2 files changed, 18 insertions(+), 2 deletions(-)
```

## RED 证据（最终测试源码 + pristine rc3 实现）

测试不是另写的简化 harness，而是上述 tag 中真实的 `packages/client/ui-file-actions/tests/files.client.spec.ts`，只加入回归及必要的生命周期用例适配。临时验证目录为 `/tmp/dsh-rc3-upstream.BL5m45/repo`，实际线上 Harness 和已安装上游包未被修改。

新增回归让 `source.read` 以 `RPC maxFileBytes exceeded (>32MiB)` 拒绝，分别覆盖：

- HTTPS + `download`
- loopback HTTP + `saveAs`

两种模式都断言原生保存成功、原始 URL 原样到达 `desktop.save`、`source.read` 未调用、`URL.createObjectURL` 未调用。

在 pristine rc3 `files.ts` 上运行：

```text
pnpm exec vitest run packages/client/ui-file-actions/tests/files.client.spec.ts
```

2026-11-24 09:44:47 +08:00 的实际结果，退出码 1：

```text
Test Files  1 failed (1)
     Tests  2 failed | 29 passed (31)

FAIL ... passes an original HTTP(S) URL to native download without reading a large source
FAIL ... passes an original HTTP(S) URL to native saveAs without reading a large source
AssertionError: promise rejected "Error: RPC maxFileBytes exceeded (>32MiB)" instead of resolving
Caused by: Error: RPC maxFileBytes exceeded (>32MiB)
 ❯ BrowserFileActions.save packages/client/ui-file-actions/src/client/files.ts:40:73
```

这正好证明失败发生在原有无条件 `source.read`，而不是原生保存或测试环境。

## GREEN 证据（相同最终测试源码 + 补丁实现）

将交付 `files.ts` 应用到同一临时 rc3 clone 后运行同一命令：

```text
pnpm exec vitest run packages/client/ui-file-actions/tests/files.client.spec.ts
```

2026-11-24 09:44:56 +08:00 的实际结果，退出码 0（提交前 09:50:10 再次重跑仍为 31/31，退出码 0）：

```text
✓ packages/client/ui-file-actions/tests/files.client.spec.ts (31 tests) 14ms

Test Files  1 passed (1)
     Tests  31 passed (31)
```

覆盖结果：

- 新增两例证明原始 HTTP(S) URL 到达原生、拒绝式 `read` 从未执行、未创建 Blob URL。
- 既有 Browser 下载、失败及资源释放用例全部通过，Browser 行为未改。
- 既有 `file-identity` 非 HTTP 用例仍通过 `read` 生成 `blob:conversion` 并撤销，证明 Desktop 非 HTTP(S) Blob 兼容路径保留。
- 上游生命周期用例的 Desktop 输入改为非 HTTP `authorized-resource`，继续覆盖“延迟 Blob 读取完成后插件已卸载则不得保存”。HTTP(S) 现在不再读取，已不适合作为该旧路径的测试输入。Browser 侧输入及断言不变。

额外尝试 `pnpm exec tsc -b packages/client/ui-file-actions/tsconfig.json --pretty false`，退出码 2。临时 rc3 clone 使用 `--ignore-scripts` 安装且未生成完整 workspace remote 契约，错误出现在 `session-controller`、`workspace-files`、`remotes` 等依赖及既有 `context-menus.tsx`，包括多处 `Cannot find module .../remote` 和缺少 remote 属性的类型错误；本次不将该命令记为通过，也不以它替代上述真实行为测试。

## 当前客户端回归

在 `/Users/zhangchengjie/Workspace/dsh-client-priority` 实际运行：

```text
pnpm typecheck
# 退出 0，无错误

pnpm test
# Test Files  12 passed (12)
# Tests       61 passed (61)
# 退出 0
```

交付测试源码使用 `.testsource` 扩展名，避免当前客户端 Vitest 把上游 jsdom 测试误当作自身测试自动发现；应用到上游时再恢复为 `tests/files.client.spec.ts`。最终重命名后已用 `cmp` 确认两个交付文件与实际通过 RED/GREEN 的 rc3 文件逐字节一致，未过滤的 `pnpm test` 于 09:54:57 实际通过，退出码 0。

## 范围与未做事项

- 提交范围仅两个补丁文件和本报告；没有修改或回退其他 dirty 文件。
- 未修改线上 Harness、`.6` worker、实际安装的上游源码、`src-tauri/src/downloads.rs`、现有桥接或其测试。
- 未部署、未构建安装包、未执行 GUI 验收、未重启任何服务。
- 补丁尚未应用到实际运行环境，因此本报告证明的是 rc3 根因及自动化回归修复，不声称 `.13` 实际下载已经恢复；应用补丁后仍需构建和真实大文件验收。
