import { describe, expect, it } from 'vitest'
import { createConnectionRecord, displayConnection, normalizeConnectionUrl, type ConnectionDraft } from '../src/connection-model'

const baseDraft: ConnectionDraft = {
  name: 'Mac worker',
  mode: 'ssh-forward',
  sshProfile: 'dsh-mac',
  remotePort: 3080,
}

describe('connection model', () => {
  it('creates a stable SSH-forward record without credentials', () => {
    const record = createConnectionRecord(baseDraft, () => 'conn-1')

    expect(record).toMatchObject({
      id: 'conn-1',
      name: 'Mac worker',
      mode: 'ssh-forward',
      sshProfile: 'dsh-mac',
      remotePort: 3080,
      partition: 'conn-1',
    })
    expect(JSON.stringify(record)).not.toMatch(/token|password|secret|privateKey/i)
  })

  it('normalizes a direct URL and rejects non-http protocols', () => {
    expect(normalizeConnectionUrl('https://dsh-mac.example.test/')).toBe('https://dsh-mac.example.test/')
    expect(() => normalizeConnectionUrl('file:///tmp/dsh')).toThrow('http')
  })

  it('redacts SSH details in display text only when present', () => {
    const record = createConnectionRecord(baseDraft, () => 'conn-1')
    expect(displayConnection(record)).toBe('Mac worker · SSH dsh-mac · :3080')
  })
})
