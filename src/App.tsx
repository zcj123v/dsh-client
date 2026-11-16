import { invoke } from '@tauri-apps/api/core'
import { useMemo, useState, type FormEvent } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import './App.css'
import { createConnectionRecord, displayConnection, type ConnectionDraft, type ConnectionRecord } from './connection-model'
import { ConnectionStore } from './connection-store'
import { connectionCapabilities } from './platform-capabilities'

const browserStorage = {
  getItem: (key: string) => window.localStorage.getItem(key),
  setItem: (key: string, value: string) => window.localStorage.setItem(key, value),
  removeItem: (key: string) => window.localStorage.removeItem(key),
}

const store = new ConnectionStore(browserStorage)
const initialDraft: ConnectionDraft = { name: '', mode: 'direct', url: 'https://', sshProfile: '', sshUser: '', remotePort: 3080 }

function App() {
  const capabilities = connectionCapabilities(navigator.userAgent)
  const [connections, setConnections] = useState<ConnectionRecord[]>(store.list())
  const [draft, setDraft] = useState<ConnectionDraft>(initialDraft)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [tokens, setTokens] = useState<Record<string, string>>({})
  const [statuses, setStatuses] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)
  const [editing, setEditing] = useState<ConnectionRecord | null>(null)

  const recent = useMemo(() => [...connections].sort((a, b) => (b.lastOpenedAt ?? '').localeCompare(a.lastOpenedAt ?? '')), [connections])

  function refresh(): void { setConnections(store.list()) }

  async function addConnection(event: FormEvent): Promise<void> {
    event.preventDefault()
    setError('')
    setNotice('')
    try {
      const record = createConnectionRecord(draft, editing ? () => editing.id : undefined)
      if (capabilities.android && (record.mode !== 'direct' || !record.url?.startsWith('https://'))) throw new Error('Android 使用已有 HTTPS 入口，不支持 SSH 或本机 HTTP')
      if (editing) { record.createdAt = editing.createdAt; record.lastOpenedAt = editing.lastOpenedAt }
      if (editing && isTauri()) await invoke("close_connection", { id: editing.id })
      store.save(record)
      refresh()
      setDraft(initialDraft)
      setEditing(null)
      setNotice('连接已保存')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '连接配置无效')
    }
  }

  async function openConnection(record: ConnectionRecord): Promise<void> {
    if (busy) return
    setBusy(true)
    setError('')
    setNotice('')
    const token = tokens[record.id]?.trim() || null
    setTokens(current => ({ ...current, [record.id]: '' }))
    setStatuses(current => ({ ...current, [record.id]: '正在连接…' }))
    try {
      if (!isTauri()) throw new Error('请在 DSH Client 应用中打开连接；浏览器页面仅用于预览')
      await invoke('open_connection', { record, token })
      store.markOpened(record.id)
      refresh()
      setStatuses(current => ({ ...current, [record.id]: '窗口已打开 · 登录状态见 DSH 页面' }))
    } catch (reason) {
      setStatuses(current => ({ ...current, [record.id]: '连接失败，可重试' }))
      setError(typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : '连接失败')
    } finally { setBusy(false) }
  }

  async function removeConnection(record: ConnectionRecord): Promise<void> {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      if (isTauri()) await invoke('delete_connection', { id: record.id })
      store.remove(record.id)
      setTokens(current => { const next = { ...current }; delete next[record.id]; return next })
      refresh()
      setNotice(capabilities.android ? `已删除 ${record.name}；已加载的浏览分区将在下次启动后请求清理` : `已删除 ${record.name}，并请求系统 WebView 清理该连接的浏览数据`)
    } catch { setError('分区清理失败，连接记录已保留，请重试') }
    finally { setBusy(false) }
  }

  function editConnection(record: ConnectionRecord): void {
    setEditing(record)
    setDraft({ ...record })
  }

  return (
    <main className="app-shell">
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
            <div><p className="section-kicker">WORKSPACES</p><h2>已保存连接</h2></div>
            <span className="count-badge">{connections.length}</span>
          </div>
          {recent.length === 0 ? <div className="empty-state"><div className="empty-icon">⌁</div><p>还没有连接</p><span>输入一个 DSH 地址，或复用 SSH 配置别名。</span></div> : (
            <div className="connection-list">
              {recent.map((record) => <article className="connection-row" key={record.id}>
                <div className="connection-mark">{record.mode === 'direct' ? '↗' : '⌘'}</div>
                <div className="connection-meta"><strong>{record.name}</strong><span>{displayConnection(record)}</span>
                  <small>{statuses[record.id] ?? '未打开'}</small>
                  <input aria-label={`${record.name} 一次性 token`} type="password" autoComplete="off" placeholder="首次登录 / 认证失效：粘贴 token（不保存）" value={tokens[record.id] ?? ''} onChange={event => { const value = event.currentTarget.value; setTokens(current => ({ ...current, [record.id]: value })) }} />
                </div>
                <div className="connection-actions"><button disabled={busy} className="primary-button compact" onClick={() => void openConnection(record)}>打开 / 重试</button><button disabled={busy} className="icon-button" title="编辑连接" onClick={() => editConnection(record)}>✎</button><button disabled={busy} className="icon-button" title="删除连接与浏览数据" onClick={() => void removeConnection(record)}>×</button></div>
              </article>)}
            </div>
          )}
        </section>

        <section className="panel add-panel">
          <div className="panel-heading"><div><p className="section-kicker">NEW CONNECTION</p><h2>{editing ? "编辑连接" : "新建连接"}</h2></div></div>
          <form className="connection-form" onSubmit={event => void addConnection(event)}>
            <label>显示名称<input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.currentTarget.value })} placeholder="Mac mini · 本机" /></label>
            <div className="segmented" role="group" aria-label="连接模式">
              <button type="button" className={draft.mode === 'direct' ? 'selected' : ''} onClick={() => setDraft({ ...draft, mode: 'direct' })}>HTTPS / Web</button>
              {capabilities.sshForward && <button type="button" className={draft.mode === 'ssh-forward' ? 'selected' : ''} onClick={() => setDraft({ ...draft, mode: 'ssh-forward' })}>SSH 转发</button>}
            </div>
            {draft.mode === 'direct' ? <label>DSH 地址<input value={draft.url} onChange={(event) => setDraft({ ...draft, url: event.currentTarget.value })} placeholder="https://dsh.example.test/" /></label> : <>
              <label>SSH 配置<select value={draft.sshConfig ?? 'default'} onChange={event => setDraft({ ...draft, sshConfig: event.currentTarget.value as 'default' | 'fleet' | 'none' })}><option value="default">~/.ssh/config</option><option value="fleet">~/.ssh/dsh-fleet-config</option><option value="none">不读取配置（IP + 用户，系统默认密钥）</option></select></label>
              <label>SSH Host 别名<input value={draft.sshProfile} onChange={(event) => setDraft({ ...draft, sshProfile: event.currentTarget.value })} placeholder="dsh-mac" /></label>
              <div className="form-row"><label>远端用户（可选）<input value={draft.sshUser} onChange={(event) => setDraft({ ...draft, sshUser: event.currentTarget.value })} placeholder="沿用 SSH 配置" /></label><label>DSH 端口<input type="number" min="1" max="65535" value={draft.remotePort} onChange={(event) => setDraft({ ...draft, remotePort: Number(event.currentTarget.value) })} /></label></div>
            </>}
            <p className="form-hint">{capabilities.android && "Android 使用独立 WebView Profile，需要系统 WebView 支持；不支持时请更新系统 WebView。"}先保存不含 token 的地址，再在左侧连接的一次性输入框粘贴 token 并打开。SSH 使用已有密钥，首次使用前请在终端确认主机指纹。</p>
            {error && <p className="form-message error">{error}</p>}
            {notice && <p className="form-message notice">{notice}</p>}
            <button disabled={busy} className="primary-button wide" type="submit">{editing ? "保存修改并关闭旧窗口" : "保存连接"}</button>{editing && <button type="button" onClick={() => { setEditing(null); setDraft(initialDraft) }}>取消编辑</button>}
          </form>
        </section>
      </section>

      <footer className="footer-note"><span>DSH Web UI 由目标节点提供</span><span>·</span><span>不启动、不托管执行体</span></footer>
    </main>
  )
}

export default App
