// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'

const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15'
const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36'

interface HarnessTab { id: string; name: string; state: 'opening' | 'ready' | 'failed'; generation: number }
interface HarnessSnapshot { tabs: HarnessTab[]; activeId: string | null }

const harness = vi.hoisted(() => {
  const h = {
    invokes: [] as Array<{ cmd: string; args: Record<string, unknown> | undefined }>,
    snapshot: { tabs: [], activeId: null } as HarnessSnapshot,
    events: {} as Record<string, ((event: { payload: unknown }) => void) | undefined>,
    hold: new Set<string>(),
    resolvers: new Map<string, Array<(value: unknown) => void>>(),
    rejectNext: new Set<string>(),
    emit: (name: string, payload: unknown) => { h.events[name]?.({ payload }) },
    release: (cmd: string, value: unknown) => {
      const pending = h.resolvers.get(cmd) ?? []
      h.resolvers.set(cmd, [])
      for (const resolve of pending) resolve(value)
    },
  }
  return h
})

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    harness.invokes.push({ cmd, args })
    if (harness.rejectNext.has(cmd)) {
      harness.rejectNext.delete(cmd)
      throw new Error('native-sync-broken')
    }
    if (cmd === 'list_tabs' || cmd === 'switch_tab' || cmd === 'close_tab') {
      if (harness.hold.has(cmd)) {
        harness.hold.delete(cmd)
        return new Promise(resolve => {
          const pending = harness.resolvers.get(cmd) ?? []
          pending.push(resolve)
          harness.resolvers.set(cmd, pending)
        })
      }
      return JSON.parse(JSON.stringify(harness.snapshot)) as typeof harness.snapshot
    }
    if (cmd === 'open_node') return { generation: 7, endpointIndex: 0 }
    if (cmd === 'close_connection' || cmd === 'delete_connection') return null
    throw new Error(`unexpected invoke ${cmd}`)
  },
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, callback: (event: { payload: unknown }) => void) => {
    harness.events[name] = callback
    return () => { delete harness.events[name] }
  },
}))

const nodeA = { id: '11111111-1111-4111-8111-111111111111', name: 'Air', mode: 'direct', url: 'http://127.0.0.1:3080/', remotePort: 3080, partition: '11111111-1111-4111-8111-111111111111', createdAt: '2026-11-16T00:00:00Z' }
const nodeB = { id: '22222222-2222-4222-8222-222222222222', name: 'k8s', mode: 'ssh-forward', sshProfile: 'k8s.example.test', remotePort: 3080, partition: '22222222-2222-4222-8222-222222222222', createdAt: '2026-11-16T00:00:00Z' }

async function renderApp(): Promise<void> {
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<App />)
  await act(async () => { })
}

function invoked(cmd: string): Array<{ cmd: string; args: Record<string, unknown> | undefined }> {
  return harness.invokes.filter(entry => entry.cmd === cmd)
}

beforeEach(() => {
  harness.invokes = []
  harness.snapshot = { tabs: [], activeId: null }
  harness.events = {}
  harness.hold.clear()
  harness.resolvers.clear()
  harness.rejectNext.clear()
  localStorage.clear()
  Object.defineProperty(window.navigator, 'userAgent', { value: MAC_UA, configurable: true })
})
afterEach(cleanup)

it('restores tabs from list_tabs after a reload without reopening nodes', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 3 }], activeId: nodeA.id }
  await renderApp()
  expect(invoked('list_tabs')).toHaveLength(1)
  expect(invoked('open_node')).toHaveLength(0)
  expect(screen.getByRole('tab', { name: /节点管理/ })).toBeTruthy()
  const airTab = screen.getByRole('tab', { name: /Air/ })
  expect(airTab.getAttribute('aria-selected')).toBe('true')
  expect(screen.getByRole('button', { name: '新建连接标签' })).toBeTruthy()
})

it('opens the chooser with existing nodes and a new-node action; plus switches to manager first', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新建连接标签' })) })
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: null })
  const chooser = screen.getByRole('dialog', { name: '选择节点' })
  expect(chooser.textContent).toContain('Air')
  expect(chooser.textContent).toContain('k8s')
  await act(async () => { fireEvent.click(within(chooser).getByRole('button', { name: /k8s/ })) })
  expect(invoked('open_node')).toHaveLength(1)
  expect(invoked('open_node')[0]!.args?.record).toMatchObject({ id: nodeB.id, name: 'k8s' })
  expect(screen.queryByRole('dialog', { name: '选择节点' })).toBeNull()
  await act(async () => {
    harness.emit('tabs-changed', { tabs: [{ id: nodeB.id, name: 'k8s', state: 'opening', generation: 7 }], activeId: nodeB.id })
  })
  expect(screen.getByRole('tab', { name: /k8s/ }).textContent).toContain('连接中')
  await act(async () => { harness.emit('tabs-changed', { tabs: [{ id: nodeB.id, name: 'k8s', state: 'ready', generation: 7 }], activeId: nodeB.id }) })
  expect(screen.getByRole('tab', { name: /k8s/ }).textContent).not.toContain('连接中')
})

it('choosing a node that already has a tab only switches and never reopens or replays a token', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: null }
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新建连接标签' })) })
  await act(async () => { fireEvent.click(within(screen.getByRole('dialog', { name: '选择节点' })).getByRole('button', { name: /Air/ })) })
  expect(invoked('open_node')).toHaveLength(0)
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: nodeA.id })
})

it('clicking an existing tab only calls switch_tab', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  harness.snapshot = {
    tabs: [
      { id: nodeA.id, name: 'Air', state: 'ready', generation: 1 },
      { id: nodeB.id, name: 'k8s', state: 'ready', generation: 2 },
    ],
    activeId: null,
  }
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /k8s/ })) })
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: nodeB.id })
  expect(invoked('open_node')).toHaveLength(0)
  expect(new Set(harness.invokes.map(entry => entry.cmd))).toEqual(new Set(['list_tabs', 'switch_tab']))
})

it('closing the active tab follows the returned native snapshot to the adjacent tab and keeps manager unclosable', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  harness.snapshot = {
    tabs: [
      { id: nodeA.id, name: 'Air', state: 'ready', generation: 1 },
      { id: nodeB.id, name: 'k8s', state: 'ready', generation: 2 },
    ],
    activeId: nodeB.id,
  }
  await renderApp()
  expect(screen.queryByRole('button', { name: '关闭标签 节点管理' })).toBeNull()
  // The native registry settles when close_tab runs, so its result already carries the post-close state.
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '关闭标签 k8s' })) })
  expect(invoked('close_tab').at(-1)?.args).toEqual({ id: nodeB.id })
  await act(async () => { })
  expect(screen.queryByRole('tab', { name: /k8s/ })).toBeNull()
  expect(screen.getByRole('tab', { name: /Air/ }).getAttribute('aria-selected')).toBe('true')
})

it('menu tab-action events drive manager, new and close through the same local handlers', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }
  await renderApp()
  await act(async () => { harness.emit('tab-action', { action: 'manager' }) })
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: null })
  await act(async () => { harness.emit('tab-action', { action: 'new' }) })
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: null })
  expect(screen.getByRole('dialog', { name: '选择节点' })).toBeTruthy()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '取消' })) })
  await act(async () => { harness.emit('tab-action', { action: 'close' }) })
  expect(invoked('close_tab').at(-1)?.args).toEqual({ id: nodeA.id })
})

it('menu close action on the fixed manager page is a no-op', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: null }
  await renderApp()
  await act(async () => { harness.emit('tab-action', { action: 'close' }) })
  expect(invoked('close_tab')).toHaveLength(0)
})

it('opening from the management page switches an existing tab unless an explicit token is provided', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '打开 / 重试' })) })
  expect(invoked('open_node')).toHaveLength(0)
  expect(invoked('switch_tab').at(-1)?.args).toEqual({ id: nodeA.id })
  await act(async () => { fireEvent.change(screen.getByLabelText('Air 一次性 token'), { target: { value: 'TOKEN_SENTINEL' } }) })
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '打开 / 重试' })) })
  expect(invoked('open_node')).toHaveLength(1)
  expect(invoked('open_node')[0]!.args?.token).toBe('TOKEN_SENTINEL')
})

it('keeps the legacy single-page UI on non-macOS platforms', async () => {
  Object.defineProperty(window.navigator, 'userAgent', { value: WINDOWS_UA, configurable: true })
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  await renderApp()
  expect(screen.queryByRole('tablist')).toBeNull()
  expect(screen.queryByRole('button', { name: '新建连接标签' })).toBeNull()
  expect(invoked('list_tabs')).toHaveLength(0)
  expect(screen.getByRole('button', { name: '打开 / 重试' })).toBeTruthy()
})

it('new node action in the chooser closes it and focuses the name field', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新建连接标签' })) })
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新增节点' })) })
  expect(screen.queryByRole('dialog', { name: '选择节点' })).toBeNull()
  expect(screen.getByLabelText('显示名称')).toBe(document.activeElement)
})

it('a slower initial list_tabs result cannot overwrite a newer tabs-changed event', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.hold.add('list_tabs')
  await renderApp()
  await act(async () => { harness.emit('tabs-changed', { tabs: [], activeId: null }) })
  await act(async () => { harness.release('list_tabs', { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 9 }], activeId: nodeA.id }) })
  expect(screen.queryByRole('tab', { name: /Air/ })).toBeNull()
})

it('a stale switch_tab result cannot overwrite a newer tabs-changed event', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: null }
  await renderApp()
  harness.hold.add('switch_tab')
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /Air/ })) })
  await act(async () => { harness.emit('tabs-changed', { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }) })
  await act(async () => { harness.release('switch_tab', { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: null }) })
  expect(screen.getByRole('tab', { name: /Air/ }).getAttribute('aria-selected')).toBe('true')
})

it('a stale close_tab result cannot overwrite a newer tabs-changed event', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA, nodeB]))
  harness.snapshot = {
    tabs: [
      { id: nodeA.id, name: 'Air', state: 'ready', generation: 1 },
      { id: nodeB.id, name: 'k8s', state: 'ready', generation: 2 },
    ],
    activeId: nodeB.id,
  }
  await renderApp()
  harness.hold.add('close_tab')
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '关闭标签 k8s' })) })
  await act(async () => { harness.emit('tabs-changed', { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }) })
  await act(async () => {
    harness.release('close_tab', {
      tabs: [
        { id: nodeA.id, name: 'Air', state: 'ready', generation: 1 },
        { id: nodeB.id, name: 'k8s', state: 'ready', generation: 2 },
      ],
      activeId: nodeB.id,
    })
  })
  expect(screen.queryByRole('tab', { name: /k8s/ })).toBeNull()
  expect(screen.getByRole('tab', { name: /Air/ }).getAttribute('aria-selected')).toBe('true')
})

it('surfaces a fixed sync error with a list_tabs retry when a tab command fails, without raw native errors', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'ready', generation: 1 }], activeId: nodeA.id }
  await renderApp()
  harness.rejectNext.add('switch_tab')
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /节点管理/ })) })
  const alert = screen.getByRole('alert')
  expect(alert.textContent).toContain('标签状态同步失败')
  expect(document.body.textContent).not.toContain('native-sync-broken')
  await act(async () => { fireEvent.click(within(alert).getByRole('button', { name: '重试同步' })) })
  expect(invoked('list_tabs')).toHaveLength(2)
  await act(async () => { })
  expect(screen.queryByRole('alert')).toBeNull()
})

it('retries a failed tab through open_node from the bar, chooser and management page instead of switching to a missing child', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'failed', generation: 1 }], activeId: null }
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /Air/ })) })
  expect(invoked('open_node')).toHaveLength(1)
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新建连接标签' })) })
  await act(async () => { fireEvent.click(within(screen.getByRole('dialog', { name: '选择节点' })).getByRole('button', { name: /Air/ })) })
  expect(invoked('open_node')).toHaveLength(2)
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '打开 / 重试' })) })
  expect(invoked('open_node')).toHaveLength(3)
  expect(invoked('switch_tab').every(entry => entry.args?.id !== nodeA.id)).toBe(true)
})

it('blocks duplicate opens while a tab is opening', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([nodeA]))
  harness.snapshot = { tabs: [{ id: nodeA.id, name: 'Air', state: 'opening', generation: 1 }], activeId: nodeA.id }
  await renderApp()
  await act(async () => { fireEvent.click(screen.getByRole('tab', { name: /Air/ })) })
  expect(invoked('open_node')).toHaveLength(0)
  expect(invoked('switch_tab').every(entry => entry.args?.id !== nodeA.id)).toBe(true)
  expect((screen.getByRole('button', { name: '打开 / 重试' }) as HTMLButtonElement).disabled).toBe(true)
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '新建连接标签' })) })
  const chooserNode = within(screen.getByRole('dialog', { name: '选择节点' })).getByRole('button', { name: /Air/ }) as HTMLButtonElement
  expect(chooserNode.textContent).toContain('连接中')
  expect(chooserNode.disabled).toBe(true)
  await act(async () => { fireEvent.click(chooserNode) })
  expect(invoked('open_node')).toHaveLength(0)
})
