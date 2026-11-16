export type ConnectionMode = 'direct' | 'ssh-forward'

export interface ConnectionDraft {
  name: string
  mode: ConnectionMode
  url?: string
  sshConfig?: 'default' | 'fleet' | 'none'
  sshProfile?: string
  sshUser?: string
  remoteHost?: string
  remotePort?: number
}

export interface ConnectionRecord {
  id: string
  name: string
  mode: ConnectionMode
  url?: string
  sshConfig?: 'default' | 'fleet' | 'none'
  sshProfile?: string
  sshUser?: string
  remoteHost?: string
  remotePort: number
  partition: string
  createdAt: string
  lastOpenedAt?: string
}

export function normalizeConnectionUrl(value: string): string {
  const input = value.trim()
  const raw = input.includes('://') ? input : `${/^(127\.0\.0\.1|localhost|\[::1\])(:|\/|$)/.test(input) ? 'http' : 'https'}://${input}`
  let parsed: URL
  try {
    parsed = new URL(raw)
  } catch {
    throw new Error('连接地址必须是 http:// 或 https:// URL')
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error('连接地址只允许 http:// 或 https://')
  }
  if (parsed.pathname !== "/") throw new Error("v1 只支持 DSH 根地址，不支持反代子路径")
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new Error('保存地址不能含认证信息、查询参数或片段；请用一次性 token 输入框登录')
  }
  if (parsed.protocol === 'http:' && !['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname)) {
    throw new Error('非本机连接必须使用 HTTPS；loopback 节点请用 SSH 转发')
  }
  return parsed.toString()
}

export function createConnectionRecord(draft: ConnectionDraft, idFactory: () => string = () => crypto.randomUUID()): ConnectionRecord {
  const id = idFactory()
  const name = draft.name.trim()
  if (!name) throw new Error('连接名称不能为空')
  if (draft.mode === 'direct') {
    if (!draft.url) throw new Error('direct 连接需要 URL')
    return {
      id, name, mode: 'direct', url: normalizeConnectionUrl(draft.url), remotePort: draft.remotePort ?? 3080,
      partition: id, createdAt: new Date().toISOString(),
    }
  }
  const sshProfile = draft.sshProfile?.trim()
  if (!sshProfile || !/^[A-Za-z0-9][A-Za-z0-9._@:-]*$/.test(sshProfile)) throw new Error('SSH Host 别名无效')
  if (draft.sshUser && !/^[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(draft.sshUser)) throw new Error('SSH 用户无效')
  if (draft.remoteHost && draft.remoteHost !== '127.0.0.1') throw new Error('SSH 目标必须为远端 loopback')
  const remotePort = draft.remotePort ?? 3080
  if (!Number.isInteger(remotePort) || remotePort < 1 || remotePort > 65535) throw new Error('远端端口无效')
  return {
    id, name, mode: 'ssh-forward', sshConfig: draft.sshConfig ?? 'default', sshProfile, sshUser: draft.sshUser?.trim() || undefined,
    remoteHost: draft.remoteHost?.trim() || '127.0.0.1', remotePort, partition: id,
    createdAt: new Date().toISOString(),
  }
}

export function displayConnection(record: ConnectionRecord): string {
  if (record.mode === 'direct') return `${record.name} · ${new URL(record.url!).host}`
  return `${record.name} · SSH ${record.sshProfile} · :${record.remotePort}`
}

export function connectionTarget(record: ConnectionRecord, localPort?: number): string {
  if (record.mode === 'direct') return record.url!
  if (localPort === undefined) throw new Error('SSH-forward 连接尚未分配本地端口')
  return `http://127.0.0.1:${localPort}/`
}
