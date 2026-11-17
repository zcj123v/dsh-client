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
  switchTab: (id: string | null) => void
  closeTab: (id: string) => void
}

/**
 * Local-shell binding for the native tab registry. Initialization reads
 * list_tabs; every later state change arrives as a full snapshot (tabs-changed
 * event or switch_tab/close_tab result) and replaces local state wholesale, so
 * the native side stays the single source of truth and stale async responses
 * cannot regress it: a command result is applied only while it is still the
 * newest command, and events always win.
 */
export function useNativeTabs(enabled: boolean, actions: NativeTabsMenuActions): NativeTabsController {
  const [snapshot, setSnapshot] = useState<TabsSnapshot | null>(null)
  const actionsRef = useRef(actions)
  actionsRef.current = actions
  const commandsRef = useRef<{ switchTab: (id: string | null) => void; closeTab: (id: string) => void }>({
    switchTab: () => { },
    closeTab: () => { },
  })

  useEffect(() => {
    if (!enabled) return
    let disposed = false
    const stops: Array<() => void> = []
    let pendingCommand = 0
    const apply = (value: unknown): void => {
      const parsed = parseTabsSnapshot(value)
      if (parsed) setSnapshot(parsed)
    }
    const run = (command: 'switch_tab' | 'close_tab', args: Record<string, unknown>): void => {
      const seq = ++pendingCommand
      void invoke<unknown>(command, args)
        .then(value => { if (!disposed && seq === pendingCommand) apply(value) })
        .catch(() => { /* Keep the last known snapshot; tabs-changed remains authoritative. */ })
    }

    void invoke<unknown>('list_tabs').then(value => { if (!disposed) apply(value) }).catch(() => { })
    void listen<unknown>('tabs-changed', event => { if (!disposed) apply(event.payload) })
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
    }
    return () => {
      disposed = true
      for (const stop of stops) stop()
    }
  }, [enabled])

  const switchTab = useCallback((id: string | null) => commandsRef.current.switchTab(id), [])
  const closeTab = useCallback((id: string) => commandsRef.current.closeTab(id), [])
  return { snapshot, switchTab, closeTab }
}
