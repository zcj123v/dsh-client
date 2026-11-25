import { invoke, isTauri } from '@tauri-apps/api/core'
import { useLayoutEffect, useRef, useState } from 'react'
import { connectionEndpoints, type ConnectionRecord } from './connection-model'
import { exportConfig, MAX_CONFIG_BYTES, parseConfig, prepareImport } from './config-transfer'

const sourceUrl = 'https://dsh-token.zcj123v.online/'
const modeTitles = { import: '导入 JSON', edit: '编辑 JSON', export: '导出 JSON' }
type Mode = keyof typeof modeTitles
export type ImportPlan = ReturnType<typeof prepareImport>

export function ConfigTransfer({ records, tokens, android, busy, apply }: {
  records: ConnectionRecord[]
  tokens: Record<string, string>
  android: boolean
  busy: boolean
  apply: (plan: ImportPlan) => Promise<void>
}) {
  const [mode, setMode] = useState<Mode | null>(null)
  const dialogRef = useRef<HTMLDialogElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const [text, setText] = useState('')
  const [nodes, setNodes] = useState<ReturnType<typeof parseConfig> | null>(null)
  const [bindings, setBindings] = useState<Record<string, string>>({})
  const [includeToken, setIncludeToken] = useState(false)
  const [confirmed, setConfirmed] = useState(false)
  const [error, setError] = useState('')
  const [status, setStatus] = useState('')
  const [working, setWorking] = useState(false)
  const disabled = busy || working
  const boundKeys = new Set(records.map(record => record.key ?? `local:${record.id}`))
  const availableTokens = records.some(record => Boolean(tokens[record.id]?.trim()))

  useLayoutEffect(() => {
    const dialog = dialogRef.current
    if (!dialog) return
    if (mode && !dialog.open) dialog.showModal()
    if (!mode && dialog.open) {
      dialog.close()
      triggerRef.current?.focus()
    }
  }, [mode])

  function openModal(next: Mode, trigger: HTMLButtonElement) {
    triggerRef.current = trigger
    show(next)
  }

  function show(next: Mode | null) {
    setMode(next)
    setText(next === 'edit' ? exportConfig(records) : '')
    setNodes(null)
    setBindings({})
    setIncludeToken(false)
    setConfirmed(false)
    setError('')
    setStatus('')
  }

  function changeText(value: string) {
    setText(value)
    setNodes(null)
    setBindings({})
    setError('')
  }

  async function readFile(file: File) {
    setWorking(true)
    try {
      if (file.size > MAX_CONFIG_BYTES) throw new Error('配置文件不能超过 1 MiB')
      const parsed = parseConfig((await file.text()).replace(/^\uFEFF/, ''))
      changeText('')
      setNodes(parsed)
    } catch (reason) { setError(reason instanceof Error ? reason.message : '读取 JSON 文件失败') }
    finally { setWorking(false) }
  }

  function preview() {
    setNodes(null)
    setError('')
    try { setNodes(parseConfig(text)) }
    catch (reason) { setError((reason as Error).message) }
  }

  async function applyPreview() {
    if (!nodes || disabled) return
    setWorking(true)
    setError('')
    try { await apply(prepareImport(nodes, records, bindings)); show(null) }
    catch (reason) { setError(reason instanceof Error ? reason.message : '未能保存节点配置') }
    finally { setWorking(false) }
  }

  async function writeConfig(output: 'file' | 'clipboard') {
    if (disabled || (includeToken && !confirmed)) return
    setWorking(true)
    setError('')
    setStatus('')
    try {
      const payload = exportConfig(records, includeToken ? tokens : {})
      if (new TextEncoder().encode(payload).length > MAX_CONFIG_BYTES) throw new Error('配置超过 1 MiB，无法导出')
      if (output === 'clipboard') {
        if (isTauri()) await invoke('copy_config_text', { text: payload })
        else await navigator.clipboard.writeText(payload)
        setStatus('配置已复制')
      } else if (isTauri()) {
        if (await invoke<boolean>('save_config_file', { text: payload })) setStatus('配置文件已保存')
      } else {
        const url = URL.createObjectURL(new Blob([payload], { type: 'application/json' }))
        const link = document.createElement('a')
        link.href = url
        link.download = 'dsh-client-config.json'
        link.click()
        setTimeout(() => URL.revokeObjectURL(url), 1000)
      }
    } catch { setError('导出失败，请检查文件或剪贴板权限') }
    finally { setWorking(false) }
  }

  return <div className="config-transfer">
    <div className="config-actions">
      <button type="button" disabled={disabled} aria-haspopup="dialog" onClick={event => openModal('import', event.currentTarget)}>导入 JSON</button>
      <button type="button" disabled={disabled} aria-haspopup="dialog" onClick={event => openModal('edit', event.currentTarget)}>编辑 JSON</button>
      <button type="button" disabled={disabled} aria-haspopup="dialog" onClick={event => openModal('export', event.currentTarget)}>导出 JSON</button>
      <a href={sourceUrl} target="_blank" rel="noreferrer" onClick={event => {
        if (isTauri()) {
          event.preventDefault()
          void invoke('open_config_source').catch(() => setError('无法打开 Token 网站'))
        }
      }}>从 Token 网站获取</a>
    </div>
    <dialog ref={dialogRef} className="config-transfer-modal" aria-labelledby="config-transfer-title" onCancel={event => {
      event.preventDefault()
      if (!disabled) show(null)
    }}>
      {mode && <>
        <header className="config-modal-heading"><h2 id="config-transfer-title">{modeTitles[mode]}</h2></header>
        <div className="config-modal-body">
          {mode === 'export' ? <>
            <p className="form-hint">备份 {records.length} 个节点的配置与入口顺序。</p>
            <label><input type="checkbox" checked={includeToken} disabled={disabled || !availableTokens} onChange={event => { setIncludeToken(event.target.checked); setConfirmed(false) }} /> 包含当前内存中的 DSH 登录 token</label>
            {includeToken && <label><input type="checkbox" checked={confirmed} disabled={disabled} onChange={event => setConfirmed(event.target.checked)} /> 我确认导出明文登录 token</label>}
          </> : <>
            {mode === 'import' && <label>选择 JSON 文件<input type="file" accept=".json,application/json" disabled={disabled} onChange={event => {
              const file = event.currentTarget.files?.[0]
              event.currentTarget.value = ''
              if (file) void readFile(file)
            }} /></label>}
            {(!nodes || (text && !nodes.some(node => node.token))) && <>
              <label>节点 JSON<textarea spellCheck={false} autoComplete="off" disabled={disabled} rows={8} value={text} onChange={event => changeText(event.target.value)} /></label>
            </>}
            {nodes && <>
              <p className="form-hint">确认入口地址后应用。未列出的本机节点会保留。入口目标有变化的节点会使用新的浏览分区，需要重新登录。</p>
              {text && nodes.some(node => node.token) && <button type="button" disabled={disabled} onClick={() => setNodes(null)}>返回编辑原文（含明文 token）</button>}
              {nodes.length === 0 && <p>这份配置没有节点。</p>}
              <ul className="config-preview">
                {nodes.map(node => {
                  const matched = boundKeys.has(node.key)
                  const previous = Object.prototype.hasOwnProperty.call(bindings, node.key) ? bindings[node.key] : ''
                  const compatible = node.endpoints.filter(entry => !android || (entry.mode === 'direct' && entry.url?.startsWith('https://'))).length
                  return <li key={node.key}>
                    <strong>{node.name}</strong> · {matched || previous ? '更新' : '新增'}{node.token ? ' · 含 token' : ''}
                    <div>{node.endpoints.map(entry => entry.mode === 'direct' ? entry.url : `${entry.sshUser ? `${entry.sshUser}@` : ''}${entry.sshProfile} → 127.0.0.1:${entry.remotePort}`).join('；')}</div>
                    {compatible !== node.endpoints.length && <small>{compatible ? 'Android 跳过 SSH 和 HTTP 入口' : 'Android 无可用的 HTTPS 入口'}</small>}
                    {!matched && records.some(record => !record.key && !nodes.some(other => other.key === `local:${record.id}`)) &&
                      <label>匹配已有节点<select value={previous} disabled={disabled} onChange={event => {
                        const value = event.target.value
                        setBindings(current => ({ ...current, [node.key]: value }))
                      }}>
                        <option value="">新建节点</option>
                        {records.filter(record => !record.key && !nodes.some(other => other.key === `local:${record.id}`)).map(record => <option key={record.id} value={record.id}>{record.name} · {connectionEndpoints(record)[0]?.label}</option>)}
                      </select></label>}
                  </li>
                })}
              </ul>
            </>}
          </>}
          {error && <p role="alert" className="form-message error">{error}</p>}
          {status && <p role="status" className="form-message notice">{status}</p>}
        </div>
        <footer className="config-modal-footer">
          <button type="button" className="secondary-button" disabled={disabled} onClick={() => show(null)}>关闭</button>
          {mode === 'export' ? <>
            <button type="button" className="secondary-button" disabled={disabled || (includeToken && !confirmed)} onClick={() => void writeConfig('clipboard')}>复制 JSON</button>
            <button type="button" className="primary-button compact" disabled={disabled || (includeToken && !confirmed)} onClick={() => void writeConfig('file')}>下载 JSON</button>
          </> : nodes ?
            <button type="button" className="primary-button compact" disabled={disabled || nodes.length === 0} onClick={() => void applyPreview()}>应用变更</button> :
            <button type="button" className="primary-button compact" disabled={disabled || !text.trim()} onClick={preview}>预览变更</button>}
        </footer>
      </>}
    </dialog>
    {!mode && error && <p role="alert" className="form-message error">{error}</p>}
  </div>
}
