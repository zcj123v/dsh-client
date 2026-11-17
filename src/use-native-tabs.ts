import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { parseTabsSnapshot, type TabsSnapshot } from './native-tabs'

export interface NativeTabsMenuActions {
  onManager: () => void
  onNew: () => void
  onCloseActive: () => void
}

export interface NativeTabsController {
  /** Latest registry snapshot; null until the first successful list_tabs or tabs-changed event. */
  snapshot: TabsSnapshot | null
  /** True when list_tabs or a tab command failed and the bar may be out of sync; retrySync re-reads list_tabs. */
  syncFailed: boolean
  retrySync: () => void
  switchTab: (id: string | null) => void
  closeTab: (id: string) => void
}

/**
 * Local-shell binding for the native tab registry. Initialization reads
 * list_tabs; every later state change arrives as a full snapshot (tabs-changed
 * event or switch_tab/close_tab/list_tabs result) and replaces local state
 * wholesale, so the native side stays the single source of truth. Stale async
 * cannot regress it: every source takes a monotonic ticket when issued, a
 * tabs-changed event always applies and invalidates every in-flight ticket,
 * and a command or list result is applied only while it is still the newest
 * ticket. Failed syncs raise syncFailed for a fixed local notice (never the
 * raw backend error) with a list_tabs retry.
 */
export function useNativeTabs(enabled: boolean, actions: NativeTabsMenuActions): NativeTabsController {
  const [snapshot, setSnapshot] = useState<TabsSnapshot | null>(null)
  const [syncFailed, setSyncFailed] = useState(false)
  const actionsRef = useRef(actions)
  actionsRef.current = actions
  const commandsRef = useRef<{ switchTab: (id: string | null) => void; closeTab: (id: string) => void; retrySync: () => void }>({
    switchTab: () => { },
    closeTab: () => { },
    retrySync: () => { },
  })

  useEffect(() => {
    if (!enabled) return
    let disposed = false
    const stops: Array<() => void> = []
    let latest = 0
    const accept = (value: unknown): void => {
      const parsed = parseTabsSnapshot(value)
      if (!parsed) return
      setSyncFailed(false)
      setSnapshot(parsed)
    }
    const guardFailure = (): void => { if (!disposed) setSyncFailed(true) }
    const syncList = (): void => {
      const ticket = ++latest
      void invoke<unknown>('list_tabs')
        .then(value => { if (!disposed && ticket === latest) accept(value) })
        .catch(guardFailure)
    }
    const run = (command: 'switch_tab' | 'close_tab', args: Record<string, unknown>): void => {
      const ticket = ++latest
      void invoke<unknown>(command, args)
        .then(value => { if (!disposed && ticket === latest) accept(value) })
        .catch(guardFailure)
    }

    syncList()
    void listen<unknown>('tabs-changed', event => {
      if (disposed) return
      ++latest // events always win: invalidate every in-flight list or command ticket
      accept(event.payload)
    })
      .then(stop => { if (disposed) stop(); else stops.push(stop) })
      .catch(() => { })
    void listen<unknown>('tab-action', event => {
      if (disposed || !event.payload || typeof event.payload !== 'object') return
      const action = (event.payload as Record<string, unknown>).action
      if (action === 'manager') actionsRef.current.onManager()
      else if (action === 'new') actionsRef.current.onNew()
      else if (action === 'close') actionsRef.current.onCloseActive()
    })
      .then(stop => { if (disposed) stop(); else stops.push(stop) })
      .catch(() => { })

    commandsRef.current = {
      switchTab: id => run('switch_tab', { id }),
      closeTab: id => run('close_tab', { id }),
      retrySync: syncList,
    }
    return () => {
      disposed = true
      for (const stop of stops) stop()
    }
  }, [enabled])

  const switchTab = useCallback((id: string | null) => commandsRef.current.switchTab(id), [])
  const closeTab = useCallback((id: string) => commandsRef.current.closeTab(id), [])
  const retrySync = useCallback(() => commandsRef.current.retrySync(), [])
  return { snapshot, syncFailed, retrySync, switchTab, closeTab }
}
