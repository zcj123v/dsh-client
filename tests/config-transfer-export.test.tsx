// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

const save = vi.fn(async () => false)
const copy = vi.fn(async () => {})
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: async (name: string, args: unknown) => name === 'save_config_file' ? save(args) : copy(args) }))
afterEach(() => { cleanup(); localStorage.clear(); save.mockClear(); copy.mockClear() })

it('exports all saved nodes by default without tokens and gates explicit plaintext token export', async () => {
  const id = '170ce412-3cbb-4467-a35a-b49fce65e8ef'
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([{ id, name: 'Mac', mode: 'direct', url: 'https://mac.example/', partition: id, remotePort: 3080, createdAt: '2026-11-16T00:00:00Z' }]))
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<App />)
  const token = screen.getByLabelText('Mac 一次性 token')
  fireEvent.change(token, { target: { value: 'fake-token' } })
  fireEvent.click(screen.getByRole('button', { name: '导出 JSON' }))
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '下载 JSON' })) })
  expect(save).toHaveBeenCalledTimes(1)
  let payload = JSON.parse((save.mock.calls[0]![0] as { text: string }).text)
  expect(payload.nodes[0].endpoints[0].url).toBe('https://mac.example/')
  expect(payload.nodes[0]).not.toHaveProperty('token')
  fireEvent.click(screen.getByLabelText('包含当前内存中的 DSH 登录 token'))
  expect(screen.getByRole('button', { name: '复制 JSON' })).toHaveProperty('disabled', true)
  fireEvent.click(screen.getByLabelText('我确认导出明文登录 token'))
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '复制 JSON' })) })
  expect(copy).toHaveBeenCalledTimes(1)
  payload = JSON.parse((copy.mock.calls[0]![0] as { text: string }).text)
  expect(payload.nodes[0].token).toBe('fake-token')
})
