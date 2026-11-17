export type NativeTabState = 'opening' | 'ready' | 'failed'

export interface NativeTab {
  id: string
  name: string
  state: NativeTabState
  generation: number
}

export interface TabsSnapshot {
  tabs: NativeTab[]
  activeId: string | null
}

/** Native tabs are enabled only on macOS, per the approved macOS-tabs design; other platforms keep the legacy UI. */
export function supportsNativeTabs(userAgent: string): boolean {
  return /Macintosh|Mac OS X/i.test(userAgent)
}

const TAB_STATES: readonly NativeTabState[] = ['opening', 'ready', 'failed']

/**
 * Validate a native tab registry snapshot (list_tabs result, switch_tab/close_tab
 * result, or tabs-changed payload). Returns null for anything that does not
 * match the contract so a malformed or foreign payload cannot poison local state.
 */
export function parseTabsSnapshot(value: unknown): TabsSnapshot | null {
  if (!value || typeof value !== 'object') return null
  const raw = value as Record<string, unknown>
  if (!Array.isArray(raw.tabs)) return null
  const tabs: NativeTab[] = []
  for (const item of raw.tabs) {
    if (!item || typeof item !== 'object') return null
    const tab = item as Record<string, unknown>
    if (typeof tab.id !== 'string' || tab.id === '') return null
    if (typeof tab.name !== 'string') return null
    if (typeof tab.state !== 'string' || !TAB_STATES.includes(tab.state as NativeTabState)) return null
    if (typeof tab.generation !== 'number' || !Number.isFinite(tab.generation)) return null
    tabs.push({ id: tab.id, name: tab.name, state: tab.state as NativeTabState, generation: tab.generation })
  }
  const activeId = raw.activeId
  if (typeof activeId !== 'string' && activeId !== null) return null
  return { tabs, activeId }
}
