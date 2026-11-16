import { describe, expect, it } from 'vitest'
import { ConnectionStore, type KeyValueStorage } from '../src/connection-store'
import type { ConnectionRecord } from '../src/connection-model'

class MemoryStorage implements KeyValueStorage {
  private values = new Map<string, string>()
  getItem(key: string): string | null { return this.values.get(key) ?? null }
  setItem(key: string, value: string): void { this.values.set(key, value) }
  removeItem(key: string): void { this.values.delete(key) }
}

const record: ConnectionRecord = {
  id: 'conn-1', name: 'Mac', mode: 'direct', url: 'http://127.0.0.1:3080/', remotePort: 3080,
  partition: 'conn-1', createdAt: '2026-11-16T00:00:00.000Z',
}

describe('ConnectionStore', () => {
  it('persists safe connection metadata and restores it', () => {
    const storage = new MemoryStorage()
    const first = new ConnectionStore(storage)
    first.save(record)
    expect(storage.getItem('dsh-client.connections.v1')).toContain('conn-1')
    expect(storage.getItem('dsh-client.connections.v1')).not.toMatch(/token|password|secret|privateKey/i)

    const second = new ConnectionStore(storage)
    expect(second.list()).toEqual([record])
  })

  it('marks a connection opened without changing its identity', () => {
    const store = new ConnectionStore(new MemoryStorage())
    store.save(record)
    store.markOpened('conn-1', '2026-11-16T01:00:00.000Z')
    expect(store.list()[0]).toMatchObject({ id: 'conn-1', lastOpenedAt: '2026-11-16T01:00:00.000Z' })
  })
})
