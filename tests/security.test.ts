import { describe, expect, it } from 'vitest'
import { createConnectionRecord, normalizeConnectionUrl } from '../src/connection-model'
import { ConnectionStore } from '../src/connection-store'

describe('connection security boundary', () => {
  it('rejects saved credentials and query strings', () => {
    for (const value of ['https://dsh.test/?token=secret', 'https://user:password@dsh.test/', 'https://dsh.test/#secret']) {
      expect(() => normalizeConnectionUrl(value)).toThrow()
    }
  })
  it('requires HTTPS outside loopback', () => {
    expect(() => normalizeConnectionUrl('http://10.250.0.13:3080/')).toThrow()
    expect(normalizeConnectionUrl('http://127.0.0.1:3080/')).toBe('http://127.0.0.1:3080/')
  })
  it('accepts a plain address without a scheme', () => {
    expect(normalizeConnectionUrl('dsh.example.test')).toBe('https://dsh.example.test/')
    expect(normalizeConnectionUrl('127.0.0.1:3080')).toBe('http://127.0.0.1:3080/')
  })
  it('rejects SSH option injection at save time', () => {
    for (const profile of ['-oProxyCommand=evil', 'bad;host', '../key']) {
      expect(() => createConnectionRecord({ name: 'test', mode: 'ssh-forward', sshProfile: profile })).toThrow()
    }
  })
  it('only serializes approved fields even when unknown credentials arrive at runtime', () => {
    const values = new Map<string, string>()
    const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value) }, removeItem: (key: string) => { values.delete(key) } }
    const store = new ConnectionStore(storage)
    const record = createConnectionRecord({ name: 'test', mode: 'direct', url: 'https://dsh.test/' })
    store.save(Object.assign(record, { token: 'SECRET_SENTINEL', password: 'PASSWORD_SENTINEL' }))
    expect(values.get('dsh-client.connections.v1')).not.toContain('SENTINEL')
    expect(new ConnectionStore(storage).list()).toEqual([createConnectionRecord({ name: 'test', mode: 'direct', url: 'https://dsh.test/' }, () => record.id)].map(r => ({ ...r, createdAt: record.createdAt })))
  })
})
