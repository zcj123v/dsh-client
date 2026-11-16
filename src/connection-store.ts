import type { ConnectionRecord } from './connection-model'

export interface KeyValueStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

const STORAGE_KEY = 'dsh-client.connections.v1'

export class ConnectionStore {
  private records: ConnectionRecord[]

  constructor(private readonly storage: KeyValueStorage) {
    this.records = readRecords(storage.getItem(STORAGE_KEY))
  }

  list(): ConnectionRecord[] { return this.records.map((record) => ({ ...record })) }

  save(record: ConnectionRecord): void {
    this.records = [...this.records.filter((item) => item.id !== record.id), { ...record }]
    this.persist()
  }

  remove(id: string): void {
    this.records = this.records.filter((record) => record.id !== id)
    this.persist()
  }

  markOpened(id: string, now = new Date().toISOString()): void {
    this.records = this.records.map((record) => record.id === id ? { ...record, lastOpenedAt: now } : record)
    this.persist()
  }

  private persist(): void {
    this.storage.setItem(STORAGE_KEY, JSON.stringify(this.records))
  }
}

function readRecords(value: string | null): ConnectionRecord[] {
  if (!value) return []
  try {
    const parsed: unknown = JSON.parse(value)
    if (!Array.isArray(parsed)) return []
    return parsed.filter(isConnectionRecord).map((record) => ({ ...record }))
  } catch {
    return []
  }
}

function isConnectionRecord(value: unknown): value is ConnectionRecord {
  if (!value || typeof value !== 'object') return false
  const record = value as Partial<ConnectionRecord>
  return typeof record.id === 'string' && typeof record.name === 'string' &&
    (record.mode === 'direct' || record.mode === 'ssh-forward') &&
    typeof record.remotePort === 'number' && typeof record.partition === 'string' && typeof record.createdAt === 'string'
}
