import { connectionEndpoints, createConnectionRecord, type ConnectionRecord, type Endpoint } from './connection-model'

export const MAX_CONFIG_BYTES = 1024 * 1024
const FORMAT = 'dsh-client-config'
const keyPattern = /^[a-zA-Z0-9][a-zA-Z0-9_.:-]{0,95}$/
const tokenPattern = /^[a-zA-Z0-9_.~-]{1,4096}$/

type ConfigNode = { key: string; name: string; endpoints: Endpoint[]; token?: string }
type JsonObject = Record<string, unknown>

function fields(value: unknown, allowed: string[], path: string): JsonObject {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${path} 必须是对象`)
  const object = value as JsonObject
  if (Object.keys(object).some(key => !allowed.includes(key))) throw new Error(`${path} 含有不支持的字段`)
  return object
}

function string(value: unknown, path: string): string {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${path} 必须是非空字符串`)
  return value
}

function parseEndpoint(value: unknown, path: string): Endpoint {
  const common = ['label', 'mode', 'kind']
  const entry = fields(value, [...common, 'url', 'sshProfile', 'sshUser', 'sshConfig', 'remotePort', 'remoteHost'], path)
  const label = string(entry.label, `${path}.label`)
  const kind = entry.kind
  if (entry.mode === 'direct') {
    fields(entry, [...common, 'url'], path)
    if (kind !== undefined && kind !== 'wg-https') throw new Error(`${path}.kind 与连接方式不匹配`)
    const url = string(entry.url, `${path}.url`)
    if (!/^https?:\/\//i.test(url) || (kind && !/^https:\/\//i.test(url))) throw new Error(`${path}.url 协议无效`)
    return { label, mode: 'direct', url, ...(kind ? { kind } : {}) }
  }
  if (entry.mode !== 'ssh-forward') throw new Error(`${path}.mode 无效`)
  fields(entry, [...common, 'sshProfile', 'sshUser', 'sshConfig', 'remotePort', 'remoteHost'], path)
  if (kind !== undefined && kind !== 'lan-ssh' && kind !== 'wg-ssh') throw new Error(`${path}.kind 与连接方式不匹配`)
  const sshConfig = entry.sshConfig ?? 'default'
  if (sshConfig !== 'default' && sshConfig !== 'fleet' && sshConfig !== 'none') throw new Error(`${path}.sshConfig 无效`)
  const sshProfile = string(entry.sshProfile, `${path}.sshProfile`)
  const sshUser = entry.sshUser === undefined ? undefined : string(entry.sshUser, `${path}.sshUser`)
  const remoteHost = entry.remoteHost ?? '127.0.0.1'
  const remotePort = entry.remotePort ?? 3080
  if (remoteHost !== '127.0.0.1') throw new Error(`${path}.remoteHost 必须为远端回环地址`)
  if (typeof remotePort !== 'number' || !Number.isInteger(remotePort) || remotePort < 1 || remotePort > 65535) throw new Error(`${path}.remotePort 无效`)
  return { label, mode: 'ssh-forward', sshProfile, sshUser, sshConfig, remoteHost, remotePort, ...(kind ? { kind } : {}) }
}

export function parseConfig(text: string): ConfigNode[] {
  if (new TextEncoder().encode(text).length > MAX_CONFIG_BYTES) throw new Error('配置文件不能超过 1 MiB')
  let parsed: unknown
  try { parsed = JSON.parse(text) } catch (error) {
    const message = error instanceof Error ? error.message : ''
    // Error text can quote the input, including a token. Report only numeric location data.
    const position = /\bposition (\d+)\b/i.exec(message)?.[1]
    const lines = position ? text.slice(0, Number(position)).split('\n') : null
    throw new Error(lines ? `JSON 语法错误（第 ${lines.length} 行，第 ${lines[lines.length - 1]!.length + 1} 列）` : 'JSON 语法错误，请检查括号、引号和逗号')
  }
  const config = fields(parsed, ['format', 'version', 'nodes'], '配置')
  if (config.format !== FORMAT || config.version !== 1) throw new Error('配置格式或版本不支持')
  if (!Array.isArray(config.nodes)) throw new Error('nodes 必须是数组')
  const keys = new Set<string>()
  return config.nodes.map((value, index) => {
    const path = `节点 ${index + 1}`
    const node = fields(value, ['key', 'name', 'endpoints', 'token'], path)
    const key = string(node.key, `${path}.key`)
    if (!keyPattern.test(key) || keys.has(key)) throw new Error(`${path}.key 无效或重复`)
    const name = string(node.name, `${path}.name`)
    if (!Array.isArray(node.endpoints) || node.endpoints.length < 1 || node.endpoints.length > 8) throw new Error(`${path} 需要 1–8 个入口`)
    const endpoints = node.endpoints.map((entry, i) => parseEndpoint(entry, `${path} 入口 ${i + 1}`))
    if (node.token != null && (typeof node.token !== 'string' || !tokenPattern.test(node.token))) throw new Error(`${path}.token 格式无效`)
    let clean: ConnectionRecord
    try { clean = createConnectionRecord({ name, key, mode: endpoints[0]!.mode, endpoints }, () => 'validation') } catch (error) {
      throw new Error(`${path} ${error instanceof Error ? error.message : '配置无效'}`)
    }
    keys.add(key)
    return { key, name: clean.name, endpoints: connectionEndpoints(clean), ...(typeof node.token === 'string' ? { token: node.token } : {}) }
  })
}

function portableEndpoint(entry: Endpoint) {
  const common = { label: entry.label, kind: entry.kind, mode: entry.mode }
  return entry.mode === 'direct' ? { ...common, url: entry.url } : {
    ...common, sshProfile: entry.sshProfile, sshUser: entry.sshUser,
    sshConfig: entry.sshConfig ?? 'default', remotePort: entry.remotePort ?? 3080,
  }
}

const stableKey = (record: ConnectionRecord) => record.key ?? `local:${record.id}`

export function exportConfig(records: ConnectionRecord[], tokens: Record<string, string> = {}): string {
  const nodes = records.map(record => {
    const token = tokens[record.id]?.trim()
    return {
      key: stableKey(record), name: record.name,
      endpoints: connectionEndpoints(record).map(portableEndpoint),
      ...(token && tokenPattern.test(token) ? { token } : {}),
    }
  })
  return JSON.stringify({ format: FORMAT, version: 1, nodes }, null, 2) + '\n'
}

function endpointTargets(record: ConnectionRecord): string[] {
  return connectionEndpoints(record).map(({ url, mode, sshConfig, sshProfile, sshUser, remotePort }) => JSON.stringify(
    mode === 'direct' ? [mode, url] : [mode, sshConfig ?? 'default', sshProfile, sshUser ?? '', remotePort ?? 3080]
  ))
}

export function endpointFingerprint(record: ConnectionRecord): string {
  return JSON.stringify(endpointTargets(record))
}

export function sameDestinations(a: ConnectionRecord, b: ConnectionRecord): boolean {
  const targets = (record: ConnectionRecord) => JSON.stringify([...new Set(endpointTargets(record))].sort())
  return targets(a) === targets(b)
}

export function prepareImport(nodes: ConfigNode[], current: ConnectionRecord[], bindings: Record<string, string> = {}) {
  const byKey = new Map(current.map(record => [stableKey(record), record]))
  const tokens: Record<string, string> = {}
  const changedRoutes: string[] = []
  const retiredIds: string[] = []
  const selected = new Set<string>()
  let updates = 0
  const records = nodes.map(node => {
    const match = byKey.get(node.key)
    const chosenId = Object.prototype.hasOwnProperty.call(bindings, node.key) ? bindings[node.key] : undefined
    const old = match ?? (chosenId ? current.find(record => record.id === chosenId && !record.key) : undefined)
    if (chosenId && !old) throw new Error('所选旧节点已变更，请重新预览')
    if (old && selected.has(old.id)) throw new Error('多个导入节点匹配了同一个本机节点')
    let record = createConnectionRecord({ key: node.key, name: node.name, mode: node.endpoints[0]!.mode, endpoints: node.endpoints })
    if (old) {
      selected.add(old.id)
      updates++
      if (sameDestinations(old, record)) record = { ...record, id: old.id, partition: old.partition }
      else retiredIds.push(old.id)
      record.createdAt = old.createdAt
      record.lastOpenedAt = old.lastOpenedAt
      if (endpointFingerprint(old) !== endpointFingerprint(record)) changedRoutes.push(old.id)
    }
    if (node.token) tokens[record.id] = node.token
    return record
  })
  return { records, tokens, changedRoutes, retiredIds, updates }
}
