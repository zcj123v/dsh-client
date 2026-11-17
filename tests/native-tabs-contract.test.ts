import { describe, expect, it } from 'vitest'
import { parseTabsSnapshot, supportsNativeTabs } from '../src/native-tabs'

describe('supportsNativeTabs', () => {
  it('enables native tabs only for macOS user agents', () => {
    expect(supportsNativeTabs('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.4 Safari/605.1.15')).toBe(true)
    expect(supportsNativeTabs('Mozilla/5.0 (Mac OS X 10.15.7) AppleWebKit/605.1.15')).toBe(true)
    expect(supportsNativeTabs('Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0')).toBe(false)
    expect(supportsNativeTabs('Mozilla/5.0 (X11; Linux x86_64) Firefox/133.0')).toBe(false)
    expect(supportsNativeTabs('Mozilla/5.0 (Linux; Android 15; Pixel) Chrome/131.0.0.0')).toBe(false)
    expect(supportsNativeTabs('Node.js/26')).toBe(false)
  })
})

describe('parseTabsSnapshot', () => {
  const valid = { tabs: [{ id: 'node-1', name: 'Air', state: 'ready', generation: 2 }], activeId: 'node-1' }
  it('accepts the native contract snapshot including a null activeId', () => {
    expect(parseTabsSnapshot(valid)).toEqual({ tabs: [{ id: 'node-1', name: 'Air', state: 'ready', generation: 2 }], activeId: 'node-1' })
    expect(parseTabsSnapshot({ tabs: [], activeId: null })).toEqual({ tabs: [], activeId: null })
  })
  it('rejects malformed snapshots so stale or foreign payloads cannot poison tab state', () => {
    expect(parseTabsSnapshot(null)).toBeNull()
    expect(parseTabsSnapshot('tabs')).toBeNull()
    expect(parseTabsSnapshot({})).toBeNull()
    expect(parseTabsSnapshot({ tabs: 'many', activeId: null })).toBeNull()
    expect(parseTabsSnapshot({ tabs: [{ id: 1, name: 'Air', state: 'ready', generation: 0 }], activeId: null })).toBeNull()
    expect(parseTabsSnapshot({ tabs: [{ id: 'node-1', name: 'Air', state: 'reconnecting', generation: 0 }], activeId: null })).toBeNull()
    expect(parseTabsSnapshot({ tabs: [{ id: 'node-1', name: 'Air', state: 'ready', generation: '3' }], activeId: null })).toBeNull()
    expect(parseTabsSnapshot({ tabs: [], activeId: 5 })).toBeNull()
    expect(parseTabsSnapshot({ tabs: [valid.tabs[0]] })).toBeNull()
    expect(parseTabsSnapshot({ generation: 100, endpointIndex: 0 })).toBeNull()
  })
})
