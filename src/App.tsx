import { invoke } from '@tauri-apps/api/core'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import './App.css'
import { connectionEndpoints, createConnectionRecord, displayConnection, type ConnectionDraft, type ConnectionRecord, type Endpoint } from './connection-model'
import { EndpointEditor, blankEndpoint } from './EndpointEditor'
import { ConnectionStore } from './connection-store'
import { connectionCapabilities } from './platform-capabilities'
import { supportsNativeTabs, type NativeTab, type NativeTabState } from './native-tabs'
import { useNativeTabs } from './use-native-tabs'
import { TabBar, TabChooser } from './TabShell'
import { ConfigTransfer, type ImportPlan } from './ConfigTransfer'
import { endpointFingerprint, sameDestinations } from './config-transfer'

const browserStorage = {
  getItem: (key: string) => window.localStorage.getItem(key),
  setItem: (key: string, value: string) => window.localStorage.setItem(key, value),
  removeItem: (key: string) => window.localStorage.removeItem(key),
}

const store = new ConnectionStore(browserStorage)
const initialDraft: ConnectionDraft = { name: '', mode: 'direct', url: 'https://', sshProfile: '', sshUser: '', remotePort: 3080 }

function App() {
  const capabilities = connectionCapabilities(navigator.userAgent)
  const nativeTabsEnabled = isTauri() && supportsNativeTabs(navigator.userAgent)
  const [connections, setConnections] = useState<ConnectionRecord[]>(store.list())
  const [draft, setDraft] = useState<ConnectionDraft>(initialDraft)
  const [entries, setEntries] = useState<Endpoint[]>([blankEndpoint()])
  const [currentEntries, setCurrentEntries] = useState<Record<string, string>>({})
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [tokens, setTokens] = useState<Record<string, string>>({})
  const [importedTokens, setImportedTokens] = useState<Record<string, string>>({})
  const [statuses, setStatuses] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)
  const openingRequest = useRef<string | null>(null)
  const generations = useRef<Record<string, number | null>>({})
  const [editing, setEditing] = useState<ConnectionRecord | null>(null)
  const [chooserOpen, setChooserOpen] = useState(false)
  const nameInputRef = useRef<HTMLInputElement | null>(null)

  const tabs = useNativeTabs(nativeTabsEnabled, {
    onManager: () => { setChooserOpen(false); tabsRef.current.switchTab(null) },
    onNew: () => { tabsRef.current.switchTab(null); setChooserOpen(true) },
    onCloseActive: () => {
      const active = tabsRef.current.snapshot?.activeId
      if (active) tabsRef.current.closeTab(active)
    },
  })
  const tabsRef = useRef(tabs)
  tabsRef.current = tabs

  // Native registry states by node id. Only 'ready' means a live child view exists:
  // 'opening' is an in-flight attempt (duplicate opens blocked) and 'failed' is a
  // registry placeholder with no child (must retry via open_node, never switch).
  const tabStates = new Map<string, NativeTabState>((tabs.snapshot?.tabs ?? []).map(tab => [tab.id, tab.state] as const))

  function selectTab(id: string): void { tabsRef.current.switchTab(id) }

  function closeTabFromBar(id: string): void { tabsRef.current.closeTab(id) }

  function handleTabSelect(tab: NativeTab): void {
    if (tab.state === 'opening') return
    if (tab.state === 'failed') {
      const record = connections.find(item => item.id === tab.id)
      if (record) void openConnection(record)
      return
    }
    selectTab(tab.id)
  }

  function showManager(): void {
    setChooserOpen(false)
    tabsRef.current.switchTab(null)
  }

  function showChooser(): void {
    tabsRef.current.switchTab(null)
    setChooserOpen(true)
  }

  function startNewNode(): void {
    setChooserOpen(false)
    setEditing(null)
    setDraft(initialDraft)
    setEntries([blankEndpoint()])
    nameInputRef.current?.focus()
  }

  useEffect(() => {
    if (!isTauri()) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<unknown>('connection-status', event => {
      if (disposed || !event.payload || typeof event.payload !== 'object') return
      const status = event.payload as Record<string, unknown>
      if (typeof status.id !== 'string' || !store.list().some(record => record.id === status.id)) return
      if (typeof status.generation !== 'number' || generations.current[status.id] !== status.generation) return
      if (typeof status.attempt !== 'number' || !Number.isInteger(status.attempt) || status.attempt < 0 || status.attempt > 3) return
      const messages: Record<string, string> = {
        reconnecting: `SSH 已断开 · 正在重连（${status.attempt}/3）`,
        ready: 'SSH 转发已恢复 · 登录状态见 DSH 页面',
        failed: 'SSH 自动重连已停止 · 点击打开 / 重试',
        disconnected: '窗口已关闭',
      }
      if (typeof status.state === 'string' && Object.prototype.hasOwnProperty.call(messages, status.state)) {
        setStatuses(current => ({ ...current, [status.id as string]: messages[status.state as string]! }))
        if (status.state === 'disconnected') setCurrentEntries(current => { const next = { ...current }; delete next[status.id as string]; return next })
      }
    }).then(stop => { if (disposed) stop(); else unlisten = stop }).catch(() => {
      if (!disposed) setNotice('无法接收连接状态；可关闭连接窗口后手动重试')
    })
    return () => { disposed = true; unlisten?.() }
  }, [])

  useEffect(() => {
    if (!isTauri() || capabilities.android) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<{ id: string; requestId: string; index: number }>('node-attempt', event => {
      if (disposed || openingRequest.current !== event.payload?.requestId) return
      const record = store.list().find(record => record.id === event.payload.id)
      const entry = record && connectionEndpoints(record)[event.payload.index]
      if (record && entry) setStatuses(current => ({ ...current, [record.id]: `正在尝试入口 ${event.payload.index + 1}：${entry.label}…` }))
    }).then(stop => { if (disposed) stop(); else unlisten = stop }).catch(() => {})
    return () => { disposed = true; unlisten?.() }
  }, [capabilities.android])

  const recent = connections

  function refresh(): void { setConnections(store.list()) }

  function clearTokens(ids: string[]): void {
    function discard(current: Record<string, string>) {
      const next = { ...current }
      for (const id of ids) delete next[id]
      return next
    }
    setTokens(discard)
    setImportedTokens(discard)
  }

  async function applyImport(plan: ImportPlan): Promise<void> {
    if (busy) throw new Error('请等待当前连接操作完成')
    setBusy(true)
    try {
      store.saveBatch(plan.records, plan.retiredIds)
      clearTokens(plan.retiredIds)
      setTokens(current => ({ ...current, ...plan.tokens }))
      setImportedTokens(current => ({ ...current, ...plan.tokens }))
      for (const id of plan.changedRoutes) delete generations.current[id]
      setCurrentEntries(current => {
        const next = { ...current }
        for (const id of plan.changedRoutes) delete next[id]
        return next
      })
      setStatuses(current => {
        const next = { ...current }
        for (const id of plan.changedRoutes) delete next[id]
        return next
      })
      setEditing(null)
      setDraft(initialDraft)
      setEntries([blankEndpoint()])
      refresh()
      setError('')
      setNotice(`已导入 ${plan.records.length - plan.updates} 个新节点，更新 ${plan.updates} 个节点`)
      if (isTauri()) {
        const closed = await Promise.allSettled(plan.changedRoutes.map(id => invoke(plan.retiredIds.includes(id) ? 'delete_connection' : 'close_connection', { id })))
        if (closed.some(result => result.status === 'rejected')) setError('配置已保存，但部分旧窗口或旧分区清理失败')
      }

    } finally { setBusy(false) }
  }

  async function addConnection(event: FormEvent): Promise<void> {
    event.preventDefault()
    if (busy) return
    setBusy(true)
    setError('')
    setNotice('')
    try {
      let record = createConnectionRecord({ ...draft, endpoints: entries })
      if (capabilities.android && !connectionEndpoints(record).some(entry => entry.mode === 'direct' && entry.url?.startsWith('https://'))) throw new Error('Android 使用已有 HTTPS 入口，不支持 SSH 或本机 HTTP')
      const retired = editing && !sameDestinations(editing, record) ? [editing.id] : []
      if (editing) {
        record.createdAt = editing.createdAt
        record.lastOpenedAt = editing.lastOpenedAt
        record.key = editing.key ?? `local:${editing.id}`
        if (!retired.length) record = { ...record, id: editing.id, partition: editing.partition }
      }
      store.saveBatch([record], retired)
      clearTokens(retired)
      refresh()
      setDraft(initialDraft)
      setEntries([blankEndpoint()])
      setEditing(null)
      setNotice(retired.length ? '连接已保存，入口目标有变化，请重新登录' : '连接已保存')
      if (editing && endpointFingerprint(editing) !== endpointFingerprint(record) && isTauri()) {
        try { await invoke(retired.length ? 'delete_connection' : 'close_connection', { id: editing.id }) }
        catch { setError('配置已保存，但旧窗口或旧分区清理失败') }
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '连接配置无效')
    } finally { setBusy(false) }
  }

  async function openConnection(record: ConnectionRecord): Promise<void> {
    const explicitToken = tokens[record.id]?.trim() || null
    // Native tabs: 'ready' means the child WebView is alive — switching only. 'opening'
    // blocks duplicate attempts. 'failed' (or no tab) has no child to switch to, so it
    // falls through to open_node retry; an explicit token is the single documented
    // reason to re-authenticate a live tab via open_node.
    if (nativeTabsEnabled && !explicitToken) {
      const tabState = tabStates.get(record.id)
      if (tabState === 'ready') {
        setError('')
        setNotice('')
        setStatuses(current => ({ ...current, [record.id]: '已有标签 · 已切换回该页面' }))
        selectTab(record.id)
        return
      }
      if (tabState === 'opening') {
        setStatuses(current => ({ ...current, [record.id]: '正在连接… 请稍候' }))
        return
      }
    }
    if (busy) return
    if (capabilities.android && !connectionEndpoints(record).some(entry => entry.mode === 'direct' && entry.url?.startsWith('https://'))) {
      setNotice('')
      setError('此节点没有 Android 可用的 HTTPS 入口')
      return
    }
    setBusy(true)
    openingRequest.current = crypto.randomUUID()
    setError('')
    setNotice('')
    setTokens(current => ({ ...current, [record.id]: '' }))
    setCurrentEntries(current => { const next = { ...current }; delete next[record.id]; return next })
    generations.current[record.id] = null
    setStatuses(current => ({ ...current, [record.id]: '正在连接…' }))
    try {
      if (!isTauri()) throw new Error('请在 DSH Client 应用中打开连接；浏览器页面仅用于预览')
      const result = await invoke<{ generation: number; endpointIndex: number }>('open_node', { record, token: explicitToken, requestId: openingRequest.current })
      generations.current[record.id] = result.generation
      setCurrentEntries(current => ({ ...current, [record.id]: connectionEndpoints(record)[result.endpointIndex]?.label ?? '默认入口' }))
      store.markOpened(record.id)
      refresh()
      setStatuses(current => ({ ...current, [record.id]: '窗口已打开 · 登录状态见 DSH 页面' }))
    } catch (reason) {
      setStatuses(current => ({ ...current, [record.id]: '连接失败，可重试' }))
      setError(typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : '连接失败')
    } finally { openingRequest.current = null; setBusy(false) }
  }

  async function removeConnection(record: ConnectionRecord): Promise<void> {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      if (isTauri()) await invoke('delete_connection', { id: record.id })
      delete generations.current[record.id]
      store.remove(record.id)
      clearTokens([record.id])
      refresh()
      setNotice(capabilities.android ? `已删除 ${record.name}；已加载的浏览分区将在下次启动后请求清理` : `已删除 ${record.name}，并请求系统 WebView 清理该连接的浏览数据`)
    } catch { setError('分区清理失败，连接记录已保留，请重试') }
    finally { setBusy(false) }
  }

  function addLocal(): void {
    if (busy) return
    const existing = store.list().find(record => connectionEndpoints(record).some(entry => entry.mode === 'direct' && entry.url === 'http://127.0.0.1:3080/'))
    if (existing) { setNotice(`本机节点已存在：${existing.name}`); return }
    store.save(createConnectionRecord({ name: '本机 DSH', mode: 'direct', endpoints: [{ label: '本机', mode: 'direct', url: 'http://127.0.0.1:3080/' }] }))
    refresh(); setError(''); setNotice('已添加本机 DSH；默认端口 3080，可点编辑修改')
  }

  function editConnection(record: ConnectionRecord): void {
    setEditing(record)
    setDraft({ ...record })
    setEntries(connectionEndpoints(record))
  }

  return (
    <>
      {nativeTabsEnabled && <TabBar snapshot={tabs.snapshot} onSelect={handleTabSelect} onManager={showManager} onClose={closeTabFromBar} onNew={showChooser} />}
      {nativeTabsEnabled && tabs.syncFailed && (
        <div className="native-tab-sync-error" role="alert">
          <span>标签状态同步失败，标签栏可能已过期</span>
          <button type="button" onClick={tabs.retrySync}>重试同步</button>
        </div>
      )}
      {nativeTabsEnabled && chooserOpen && <TabChooser connections={connections} tabStates={tabStates} onChoose={record => { setChooserOpen(false); void openConnection(record) }} onNewNode={startNewNode} onDismiss={() => setChooserOpen(false)} />}
      <main className={nativeTabsEnabled ? 'app-shell with-tabs' : 'app-shell'}>
        <header className="topbar">
        <div>
          <p className="eyebrow">DSH CLIENT</p>
          <h1>连接到正在运行的 DSH</h1>
          <p className="subtitle">客户端只负责连接和窗口，执行过程始终留在目标节点。</p>
        </div>
        <div className="status-pill"><span className="status-dot" />纯客户端模式</div>
      </header>

      <section className="workspace-grid">
        <section className="panel connections-panel">
          <div className="panel-heading">
            <div><p className="section-kicker">WORKSPACES</p><h2>已保存节点</h2></div>
            <span className="count-badge">{connections.length}</span>
          </div>
          {!capabilities.android && <div className="local-shortcut"><button disabled={busy} className="secondary-button" onClick={addLocal}>添加本机 DSH</button><small>127.0.0.1:3080 · 不启动服务</small></div>}
          <ConfigTransfer records={connections} tokens={{ ...importedTokens, ...Object.fromEntries(Object.entries(tokens).filter(([, token]) => token.trim())) }} android={capabilities.android} busy={busy} apply={applyImport} />
          {recent.length === 0 ? <div className="empty-state"><div className="empty-icon">⌁</div><p>还没有连接</p><span>输入一个 DSH 地址，或复用 SSH 配置别名。</span></div> : (
            <div className="connection-list">
              {recent.map((record) => <article className="connection-row" key={record.id}>
                <div className="connection-mark">{record.mode === 'direct' ? '↗' : '⌘'}</div>
                <div className="connection-meta"><strong>{record.name}</strong><span>{displayConnection(record)}</span>
                  <span className="entry-chain">{connectionEndpoints(record).map(entry => entry.label).join(" → ")}</span>
                  {currentEntries[record.id] && <small>当前入口：{currentEntries[record.id]}</small>}
                  <small role="status">{statuses[record.id] ?? '未打开'}</small>
                  {importedTokens[record.id] && <button type="button" disabled={busy} onClick={() => clearTokens([record.id])}>清除 {record.name} 的内存 token</button>}
                  <input aria-label={`${record.name} 一次性 token`} type="password" autoComplete="off" placeholder="首次登录 / 认证失效：粘贴 token（不保存）" value={tokens[record.id] ?? ''} onChange={event => { const value = event.currentTarget.value; setTokens(current => ({ ...current, [record.id]: value })) }} />
                </div>
                <div className="connection-actions"><button disabled={busy || (nativeTabsEnabled && tabStates.get(record.id) === 'opening' && !tokens[record.id]?.trim())} className="primary-button compact" onClick={() => void openConnection(record)}>打开 / 重试</button><button disabled={busy} className="icon-button" title="编辑连接" onClick={() => editConnection(record)}>✎</button><button disabled={busy} className="icon-button" title="删除连接与浏览数据" onClick={() => void removeConnection(record)}>×</button></div>
              </article>)}
            </div>
          )}
        </section>

        <section className="panel add-panel">
          <div className="panel-heading"><div><p className="section-kicker">NEW CONNECTION</p><h2>{editing ? "编辑节点" : "新建节点"}</h2></div></div>
          <form className="connection-form" onSubmit={event => void addConnection(event)}>
            <label>显示名称<input ref={nameInputRef} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.currentTarget.value })} placeholder="Mac mini · 本机" /></label>
            <p className="form-hint">入口从上往下依次尝试，建议 LAN → WG。已打开窗口保持当前入口；关闭重开时重新按顺序选择。</p>
            <EndpointEditor entries={entries} setEntries={setEntries} ssh={capabilities.sshForward} multiple={!capabilities.android} />
            <p className="form-hint">{capabilities.android && "Android 使用独立 WebView Profile，需要系统 WebView 支持；不支持时请更新系统 WebView。"}先保存不含 token 的地址，再在左侧连接的一次性输入框粘贴 token 并打开。SSH 使用已有密钥，首次使用前请在终端确认主机指纹。</p>
            {error && <p className="form-message error">{error}</p>}
            {notice && <p className="form-message notice">{notice}</p>}
            <button disabled={busy} className="primary-button wide" type="submit">{editing ? "保存修改并关闭旧窗口" : "保存节点"}</button>{editing && <button type="button" onClick={() => { setEditing(null); setDraft(initialDraft); setEntries([blankEndpoint()]) }}>取消编辑</button>}
          </form>
        </section>
      </section>

      <footer className="footer-note"><span>DSH Web UI 由目标节点提供</span><span>·</span><span>不启动、不托管执行体</span></footer>
      </main>
    </>
  )
}

export default App
