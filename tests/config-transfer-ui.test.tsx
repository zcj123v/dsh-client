// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => false, invoke: vi.fn() }))
afterEach(() => { cleanup(); localStorage.clear() })

it('opens each transfer action in a modal and restores focus when closed', async () => {
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<App />)
  for (const name of ['导入 JSON', '编辑 JSON', '导出 JSON']) {
    const trigger = screen.getByRole('button', { name })
    trigger.focus()
    fireEvent.click(trigger)
    const dialog = screen.getByRole('dialog', { name }) as HTMLDialogElement
    expect(dialog.open).toBe(true)
    if (name === '编辑 JSON') expect(within(dialog).getByLabelText('节点 JSON')).toBeDefined()
    if (name === '导出 JSON') fireEvent(dialog, new Event('cancel', { cancelable: true }))
    else fireEvent.click(within(dialog).getByRole('button', { name: '关闭' }))
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  }
})

it('previews before saving, retains token only in memory, and edits the full JSON backup', async () => {
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<App />)
  fireEvent.click(screen.getByRole('button', { name: '添加本机 DSH' }))
  fireEvent.click(screen.getByRole('button', { name: '导入 JSON' }))
  fireEvent.change(screen.getByLabelText('节点 JSON'), { target: { value: JSON.stringify({ format: 'dsh-client-config', version: 1, nodes: [{ key: 'air', name: 'Air', endpoints: [{ mode: 'direct', label: 'WG', url: 'https://air.example/' }], token: 'fake-login-token' }] }) } })
  fireEvent.click(screen.getByRole('button', { name: '预览变更' }))
  expect(screen.queryByLabelText('Air 一次性 token')).toBeNull()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '应用变更' })) })
  expect((screen.getByLabelText('Air 一次性 token') as HTMLInputElement).value).toBe('fake-login-token')
  expect(localStorage.getItem('dsh-client.connections.v1')).not.toContain('fake-login-token')
  expect(screen.getByText('本机 DSH · 127.0.0.1:3080')).toBeDefined()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '编辑 JSON' })) })
  const editor = screen.getByLabelText('节点 JSON') as HTMLTextAreaElement
  expect(editor.value).toContain('http://127.0.0.1:3080/')
  expect(editor.value).not.toContain('fake-login-token')
  fireEvent.change(editor, { target: { value: editor.value.replace('"Air"', '"Air edited"') } })
  fireEvent.click(screen.getByRole('button', { name: '预览变更' }))
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '应用变更' })) })
  expect((screen.getByLabelText('Air edited 一次性 token') as HTMLInputElement).value).toBe('fake-login-token')

  const storageKey = 'dsh-client.connections.v1'
  const original = JSON.parse(localStorage.getItem(storageKey)!).find((node: { key?: string }) => node.key === 'air')
  fireEvent.click(screen.getByRole('button', { name: '编辑 JSON' }))
  const targetEditor = screen.getByLabelText('节点 JSON') as HTMLTextAreaElement
  fireEvent.change(targetEditor, { target: { value: targetEditor.value.replace('https://air.example/', 'https://new-air.example/') } })
  fireEvent.click(screen.getByRole('button', { name: '预览变更' }))
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '应用变更' })) })
  const updated = JSON.parse(localStorage.getItem(storageKey)!).filter((node: { key?: string }) => node.key === 'air')
  expect(updated).toHaveLength(1)
  expect(updated[0].id).not.toBe(original.id)
  expect((screen.getByLabelText('Air edited 一次性 token') as HTMLInputElement).value).toBe('')
})
