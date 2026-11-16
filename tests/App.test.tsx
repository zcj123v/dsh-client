// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StrictMode } from 'react'

const nativeEvents = vi.hoisted(() => ({ receive: undefined as undefined | ((event: { payload: unknown }) => void) }))
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: async () => 100 }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async (_name: string, callback: (event: { payload: unknown }) => void) => { nativeEvents.receive = callback; return () => { nativeEvents.receive = undefined } } }))

afterEach(cleanup)
it('shows bounded native SSH recovery and failure without persisting lifecycle or error secrets', async () => {
  const id = '170ce412-3cbb-4467-a35a-b49fce65e8ef'
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([{ id, name:'Recovery', mode:'ssh-forward', sshProfile:'example.test', remotePort:3080, partition:id, createdAt:'2026-11-16T00:00:00Z' }]))
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<App />)
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '打开 / 重试' })) })
  await act(async () => { nativeEvents.receive?.({ payload: { id, generation: 100, state: 'reconnecting', attempt: 2, message: 'NEVER_RENDER_SECRET' } }) })
  expect(screen.getByText('SSH 已断开 · 正在重连（2/3）')).toBeDefined()
  await act(async () => { nativeEvents.receive?.({ payload: { id, generation: 100, state: 'ready', attempt: 2 } }) })
  expect(screen.getByText('SSH 转发已恢复 · 登录状态见 DSH 页面')).toBeDefined()
  await act(async () => { nativeEvents.receive?.({ payload: { id, generation: 100, state: 'failed', attempt: 3 } }) })
  expect(screen.getByText('SSH 自动重连已停止 · 点击打开 / 重试')).toBeDefined()
  await act(async () => { nativeEvents.receive?.({ payload: { id, generation: 99, state: 'ready', attempt: 1 } }) })
  expect(screen.getByText('SSH 自动重连已停止 · 点击打开 / 重试')).toBeDefined()
  expect(document.body.textContent).not.toContain('NEVER_RENDER_SECRET')
  expect(localStorage.getItem('dsh-client.connections.v1')).not.toContain('reconnecting')
})
it('keeps the token input usable after React releases the input event and never saves it', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([{ id:'170ce412-3cbb-4467-a35a-b49fce65e8ef', name:'Test', mode:'direct', url:'http://127.0.0.1:3080/', remotePort:3080, partition:'170ce412-3cbb-4467-a35a-b49fce65e8ef', createdAt:'2026-11-16T00:00:00Z' }]))
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<StrictMode><App /></StrictMode>)
  const input = screen.getByLabelText('Test 一次性 token') as HTMLInputElement
  fireEvent.change(input, { target: { value: 'TEST_SENTINEL' } })
  fireEvent.change(input, { target: { value: 'TEST_SENTINEL_TWO' } })
  expect(input.value).toBe('TEST_SENTINEL_TWO')
  expect(localStorage.getItem('dsh-client.connections.v1')).not.toContain('SENTINEL')
})
