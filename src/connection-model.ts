export type ConnectionMode = 'direct' | 'ssh-forward'

export interface ConnectionDraft {
  name: string
  mode: ConnectionMode
  url?: string
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
  sshProfile?: string
  sshUser?: string
  remoteHost?: string
  remotePort: number
  partition: string
  createdAt: string
  lastOpenedAt?: string
}

export function normalizeConnectionUrl(value: string): string {
  const raw = value.trim()
  let parsed: URL
  try {
    parsed = new URL(raw)
  } catch {
    throw new Error('连接地址必须是 http:// 或 https:// URL')
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error('连接地址只允许 http:// 或 https://')
  }
  parsed.hash = ''
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
  if (!sshProfile) throw new Error('SSH-forward 连接需要 SSH Host 别名')
  const remotePort = draft.remotePort ?? 3080
  if (!Number.isInteger(remotePort) || remotePort < 1 || remotePort > 65535) throw new Error('远端端口无效')
  return {
    id, name, mode: 'ssh-forward', sshProfile, sshUser: draft.sshUser?.trim() || undefined,
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
