import { createConnectionRecord, type ConnectionRecord } from './connection-model'

export interface KeyValueStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}
const STORAGE_KEY = 'dsh-client.connections.v1'

function sanitizeRecord(record: ConnectionRecord): ConnectionRecord {
  const clean = createConnectionRecord(record, () => record.id)
  clean.partition = record.id
  clean.createdAt = record.createdAt
  if (typeof record.lastOpenedAt === 'string') clean.lastOpenedAt = record.lastOpenedAt
  return clean
}
export class ConnectionStore {
  private records: ConnectionRecord[] = []
  constructor(private readonly storage: KeyValueStorage) {
    try {
      const parsed: unknown = JSON.parse(storage.getItem(STORAGE_KEY) ?? '[]')
      if (Array.isArray(parsed)) {
        for (const value of parsed) {
          try {
            if (typeof value?.id !== 'string' || !/^[A-Za-z0-9-]{1,64}$/.test(value.id) || typeof value.createdAt !== 'string') continue
            this.records.push(sanitizeRecord(value))
          } catch { /* Ignore malformed or credential-bearing old records. */ }
        }
      }
    } catch { /* Start empty when storage is corrupt. */ }
  }
  list(): ConnectionRecord[] { return this.records.map(record => ({ ...record })) }
  save(record: ConnectionRecord): void {
    const clean = sanitizeRecord(record)
    this.records = [...this.records.filter(item => item.id !== record.id), clean]
    this.persist()
  }
  remove(id: string): void {
    this.records = this.records.filter(record => record.id !== id)
    this.persist()
  }
  markOpened(id: string, now = new Date().toISOString()): void {
    this.records = this.records.map(record => record.id === id ? { ...record, lastOpenedAt: now } : record)
    this.persist()
  }
  private persist(): void { this.storage.setItem(STORAGE_KEY, JSON.stringify(this.records)) }
}
