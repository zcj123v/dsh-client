import { invoke } from '@tauri-apps/api/core'
import { useMemo, useState, type FormEvent } from 'react'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import './App.css'
import { createConnectionRecord, displayConnection, type ConnectionDraft, type ConnectionRecord } from './connection-model'
import { ConnectionStore } from './connection-store'
import { buildSshForwardArgs } from './ssh-forward'
import { webviewIsolationOptions } from './webview-isolation'

const browserStorage = {
  getItem: (key: string) => window.localStorage.getItem(key),
  setItem: (key: string, value: string) => window.localStorage.setItem(key, value),
  removeItem: (key: string) => window.localStorage.removeItem(key),
}

const store = new ConnectionStore(browserStorage)
const initialDraft: ConnectionDraft = { name: '', mode: 'direct', url: 'https://', sshProfile: '', sshUser: '', remotePort: 3080 }

function App() {
  const [connections, setConnections] = useState<ConnectionRecord[]>(store.list())
  const [draft, setDraft] = useState<ConnectionDraft>(initialDraft)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')

  const recent = useMemo(() => [...connections].sort((a, b) => (b.lastOpenedAt ?? '').localeCompare(a.lastOpenedAt ?? '')), [connections])

  function refresh(): void { setConnections(store.list()) }

  function addConnection(event: FormEvent): void {
    event.preventDefault()
    setError('')
    setNotice('')
    try {
      const record = createConnectionRecord(draft)
      store.save(record)
      refresh()
      setDraft(initialDraft)
      setNotice('连接已保存')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '连接配置无效')
    }
  }

  async function openConnection(record: ConnectionRecord): Promise<void> {
    setError('')
    setNotice('')
    store.markOpened(record.id)
    refresh()
    if (record.mode === 'direct') {
      const label = `dsh-${record.id}`
      try {
        const existing = await WebviewWindow.getByLabel(label)
        if (existing) { await existing.setFocus(); return }
        const view = new WebviewWindow(label, {
          url: record.url,
          title: `DSH · ${record.name}`,
          width: 1440,
          height: 920,
          resizable: true,
          ...webviewIsolationOptions(record),
        })
        await view.once('tauri://error', (event) => setError(`打开 DSH 失败：${String(event.payload ?? 'WebView 创建失败')}`))
      } catch {
        window.open(record.url, '_blank', 'noopener,noreferrer')
      }
      return
    }
    try {
      const localPort = await invoke<number>('allocate_local_port')
      const args = buildSshForwardArgs({ profile: record.sshProfile!, user: record.sshUser, remotePort: record.remotePort, localPort })
      await invoke('start_ssh_forward', { args })
      const label = `dsh-${record.id}`
      const existing = await WebviewWindow.getByLabel(label)
      if (existing) { await existing.setFocus(); return }
      const view = new WebviewWindow(label, {
        url: `http://127.0.0.1:${localPort}/`,
        title: `DSH · ${record.name}`,
        width: 1440,
        height: 920,
        resizable: true,
        ...webviewIsolationOptions(record),
      })
      await view.once('tauri://error', (event) => setError(`打开 DSH 失败：${String(event.payload ?? 'WebView 创建失败')}`))
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '无法启动 SSH 转发；请检查 SSH Host 别名和 known_hosts。')
    }
  }

  function removeConnection(record: ConnectionRecord): void {
    store.remove(record.id)
    refresh()
    setNotice(`已删除 ${record.name}；再次连接需要重新认证`)
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
                <div className="connection-meta"><strong>{record.name}</strong><span>{displayConnection(record)}</span></div>
                <div className="connection-actions"><button className="primary-button compact" onClick={() => void openConnection(record)}>打开</button><button className="icon-button" title="删除连接" onClick={() => removeConnection(record)}>×</button></div>
              </article>)}
            </div>
          )}
        </section>

        <section className="panel add-panel">
          <div className="panel-heading"><div><p className="section-kicker">NEW CONNECTION</p><h2>新建连接</h2></div></div>
          <form className="connection-form" onSubmit={addConnection}>
            <label>显示名称<input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.currentTarget.value })} placeholder="Mac mini · 本机" /></label>
            <div className="segmented" role="group" aria-label="连接模式">
              <button type="button" className={draft.mode === 'direct' ? 'selected' : ''} onClick={() => setDraft({ ...draft, mode: 'direct' })}>HTTPS / Web</button>
              <button type="button" className={draft.mode === 'ssh-forward' ? 'selected' : ''} onClick={() => setDraft({ ...draft, mode: 'ssh-forward' })}>SSH 转发</button>
            </div>
            {draft.mode === 'direct' ? <label>DSH 地址<input value={draft.url} onChange={(event) => setDraft({ ...draft, url: event.currentTarget.value })} placeholder="https://dsh.example.test/" /></label> : <>
              <label>SSH Host 别名<input value={draft.sshProfile} onChange={(event) => setDraft({ ...draft, sshProfile: event.currentTarget.value })} placeholder="dsh-mac" /></label>
              <div className="form-row"><label>远端用户（可选）<input value={draft.sshUser} onChange={(event) => setDraft({ ...draft, sshUser: event.currentTarget.value })} placeholder="沿用 SSH 配置" /></label><label>DSH 端口<input type="number" min="1" max="65535" value={draft.remotePort} onChange={(event) => setDraft({ ...draft, remotePort: Number(event.currentTarget.value) })} /></label></div>
            </>}
            <p className="form-hint">首次打开时，在目标 DSH 根页面粘贴启动 token；客户端不会保存 token 或密码。</p>
            {error && <p className="form-message error">{error}</p>}
            {notice && <p className="form-message notice">{notice}</p>}
            <button className="primary-button wide" type="submit">保存连接</button>
          </form>
        </section>
      </section>

      <footer className="footer-note"><span>DSH Web UI 由目标节点提供</span><span>·</span><span>不启动、不托管执行体</span></footer>
    </main>
  )
}

export default App
