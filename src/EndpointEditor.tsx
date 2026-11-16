import { type Endpoint } from './connection-model'

export const blankEndpoint = (label = 'LAN'): Endpoint => ({ label, mode: 'direct', url: 'https://', sshConfig: 'default', sshProfile: '', sshUser: '', remotePort: 3080 })

export function EndpointEditor({ entries, setEntries, ssh, multiple }: { entries: Endpoint[]; setEntries: (entries: Endpoint[]) => void; ssh: boolean; multiple: boolean }) {
  const update = (index: number, patch: Partial<Endpoint>) => setEntries(entries.map((entry, i) => i === index ? { ...entry, ...patch } : entry))
  function move(index: number, offset: number) {
    const next = [...entries]; const target = index + offset
    ;[next[index], next[target]] = [next[target]!, next[index]!]
    setEntries(next)
  }
  return <div className="endpoint-list">
    {entries.map((entry, i) => <fieldset className="endpoint-editor" key={i}>
      <legend>入口 {i + 1}{i === 0 ? ' · 优先连接' : ' · 备用'}</legend>
      {multiple && <div className="endpoint-toolbar">
        <button type="button" aria-label={`上移入口 ${i + 1}`} disabled={i === 0} onClick={() => move(i, -1)}>↑ 上移</button>
        <button type="button" aria-label={`下移入口 ${i + 1}`} disabled={i === entries.length - 1} onClick={() => move(i, 1)}>↓ 下移</button>
        <button type="button" aria-label={`删除入口 ${i + 1}`} disabled={entries.length === 1} onClick={() => setEntries(entries.filter((_, n) => n !== i))}>移除</button>
      </div>}
      <label>入口名称<input aria-label={`入口 ${i + 1} 名称`} value={entry.label} onChange={event => update(i, { label: event.currentTarget.value })} placeholder={i === 0 ? 'LAN' : 'WG'} maxLength={40} /></label>
      <div className="segmented" role="group" aria-label={`入口 ${i + 1} 连接模式`}>
        <button type="button" className={entry.mode === 'direct' ? 'selected' : ''} onClick={() => update(i, {mode:'direct'})}>HTTPS / 本机 Web</button>
        {ssh && <button type="button" className={entry.mode === 'ssh-forward' ? 'selected' : ''} onClick={() => update(i, {mode:'ssh-forward'})}>SSH 转发</button>}
      </div>
      {entry.mode === 'direct' ? <label>DSH 地址<input aria-label={`入口 ${i + 1} DSH 地址`} value={entry.url ?? ''} onChange={event => update(i, {url:event.currentTarget.value})} placeholder="https://dsh.example.test/" /></label> : <>
        <label>SSH 配置<select aria-label={`入口 ${i + 1} SSH 配置`} value={entry.sshConfig ?? 'default'} onChange={event => update(i, {sshConfig:event.currentTarget.value as Endpoint['sshConfig']})}><option value="default">~/.ssh/config</option><option value="fleet">~/.ssh/dsh-fleet-config</option><option value="none">IP + 用户（系统默认密钥）</option></select></label>
        <label>SSH Host / IP<input aria-label={`入口 ${i + 1} SSH Host`} value={entry.sshProfile ?? ''} onChange={event => update(i, {sshProfile:event.currentTarget.value})} placeholder={i === 0 ? '192.168.1.20 或 LAN 别名' : '10.250.0.13 或 WG 别名'} /></label>
        <div className="form-row"><label>远端用户<input aria-label={`入口 ${i + 1} SSH 用户`} value={entry.sshUser ?? ''} onChange={event => update(i, {sshUser:event.currentTarget.value})} placeholder="沿用 SSH 配置" /></label><label>DSH 端口<input aria-label={`入口 ${i + 1} DSH 端口`} type="number" min="1" max="65535" value={entry.remotePort ?? 3080} onChange={event => update(i, {remotePort:Number(event.currentTarget.value)})} /></label></div>
      </>}
    </fieldset>)}
    {multiple && <button type="button" className="secondary-button" disabled={entries.length >= 8} onClick={() => setEntries([...entries, blankEndpoint(entries.length === 1 ? 'WG' : `备用 ${entries.length}`)])}>添加备用入口</button>}
  </div>
}
