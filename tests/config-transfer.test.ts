import { expect, test } from 'vitest'
import { createConnectionRecord } from '../src/connection-model'
import { ConnectionStore } from '../src/connection-store'
import { exportConfig, parseConfig, prepareImport } from '../src/config-transfer'

const endpoint = { label: '本机', mode: 'direct', url: 'http://127.0.0.1:3080/' } as const
const bundle = (nodes: unknown[]) => JSON.stringify({ format: 'dsh-client-config', version: 1, nodes })
const node = { key: 'macmini', name: 'Mac Mini', endpoints: [endpoint] }

test('round trips localhost and ordered SSH entries without runtime fields or default credentials', () => {
  const record = createConnectionRecord({ ...node, mode: 'direct', endpoints: [endpoint, { kind: 'wg-ssh', label: 'WG', mode: 'ssh-forward', sshProfile: '10.250.0.6', sshUser: 'user', remotePort: 3080 }] })
  const tokens = { [record.id]: 'fake-token' }
  const exported = exportConfig([record])
  expect(exported).not.toContain('partition')
  expect(exported).not.toContain('fake-token')
  expect(parseConfig(exported)[0]).toMatchObject({ key: 'macmini', endpoints: [{ url: endpoint.url }, { kind: 'wg-ssh', sshProfile: '10.250.0.6' }] })
  expect(parseConfig(exportConfig([record], tokens))[0]?.token).toBe('fake-token')
})

test('rejects duplicate keys, unsafe fields and addresses without echoing sensitive input', () => {
  for (const nodes of [[node, node], [{ ...node, password: 'never-echo' }], [{ ...node, token: 'invalid never-echo' }], [{ ...node, endpoints: [{ ...endpoint, url: 'https://example.test/?token=never-echo' }] }], [{ ...node, endpoints: [{ label: 'SSH', mode: 'ssh-forward', sshProfile: '-F' }] }]]) {
    let error: unknown
    try { parseConfig(bundle(nodes)) } catch (reason) { error = reason }
    expect(error).toBeInstanceOf(Error)
    expect(String(error)).not.toContain('never-echo')
  }
})

test('upserts by stable key and changes neither saved records nor memory on a failed write', () => {
  let fail = false
  let saved: string | null = null
  const store = new ConnectionStore({ getItem: () => saved, setItem: (_, value) => { if (fail) throw new Error('disk'); saved = value }, removeItem() {} })
  const old = createConnectionRecord({ ...node, ...endpoint })
  const other = createConnectionRecord({ ...endpoint, name: 'Other' })
  store.save(old)
  store.save(other)
  const plan = prepareImport(parseConfig(bundle([{ ...node, name: 'Renamed', token: 'fake-token' }])), store.list())
  expect(plan.records[0]).toMatchObject({ id: old.id, partition: old.partition, name: 'Renamed' })
  expect(plan.tokens[old.id]).toBe('fake-token')
  fail = true
  expect(() => store.saveBatch(plan.records)).toThrow('disk')
  expect(store.list()[0]?.name).toBe('Mac Mini')
  fail = false
  store.saveBatch(plan.records)
  expect(store.list().map(x => x.id)).toEqual([old.id, other.id])
  expect(saved).not.toContain('fake-token')
})

test('rotates the browser partition only when the set of destinations changes', () => {
  const ssh = { mode: 'ssh-forward', label: 'WG', sshProfile: '10.250.0.6' } as const
  const old = createConnectionRecord({ ...node, mode: 'direct', endpoints: [endpoint, ssh] })
  const same = prepareImport(parseConfig(bundle([{ ...node, endpoints: [ssh, endpoint] }])), [old])
  expect(same.records[0]?.id).toBe(old.id)
  expect(same.changedRoutes).toEqual([old.id])
  const changed = prepareImport(parseConfig(bundle([{ ...node, endpoints: [endpoint, { ...ssh, sshProfile: '10.250.0.7' }] }])), [old])
  expect(changed.records[0]?.id).not.toBe(old.id)
  expect(changed.records[0]?.partition).toBe(changed.records[0]?.id)
  expect(changed.retiredIds).toEqual([old.id])
  const store = new ConnectionStore({ getItem: () => null, setItem() {}, removeItem() {} })
  store.save(old)
  store.saveBatch(changed.records, changed.retiredIds)
  expect(store.list().map(x => x.id)).toEqual([changed.records[0]?.id])
})

test('rejects empty query and fragment delimiters as well as populated ones', () => {
  for (const suffix of ['?', '#']) {
    expect(() => parseConfig(bundle([{ ...node, endpoints: [{ ...endpoint, url: endpoint.url + suffix }] }]))).toThrow()
  }
})

test('a form-edited endpoint exports without an incompatible website kind', () => {
  const record = createConnectionRecord({ ...node, mode: 'direct', endpoints: [{ ...endpoint, kind: 'wg-ssh' }] })
  expect(parseConfig(exportConfig([record]))[0]?.endpoints[0]).not.toHaveProperty('kind')
})

test('rejects kind tags incompatible with the endpoint mode or URL scheme', () => {
  for (const entry of [
    { ...endpoint, kind: 'wg-https', url: 'http://evil.example/' },
    { ...endpoint, kind: 'wg-https', url: 'http://127.0.0.1:3080/' },
    { ...endpoint, kind: 'wg-ssh' },
    { ...endpoint, kind: 'unknown-kind' },
    { label: 'WG', mode: 'ssh-forward', kind: 'wg-https', sshProfile: '10.250.0.6' },
    { label: 'LAN', mode: 'ssh-forward', kind: 'unknown-kind', sshProfile: 'host' },
  ]) {
    expect(() => parseConfig(bundle([{ ...node, endpoints: [entry] }]))).toThrow()
  }
})
