// @ts-nocheck -- Rust executes this JavaScript-compatible .ts source directly.
/**
 * Minimal desktop adapter injected only into macOS node content WebViews.
 * This file intentionally stays valid JavaScript after its module exports are
 * removed, so Rust can initialize the exact tested implementation natively.
 *
 * @typedef {{url: string, name?: string, mediaType?: string, mode: 'download' | 'saveAs'}} SaveRequest
 * @typedef {{status: 'cancelled'} | {status: 'saved', path: string}} SaveOutcome
 * @typedef {{save(request: SaveRequest): Promise<SaveOutcome>, readClipboardImage(url: string): Promise<never>}} DesktopFileActions
 * @typedef {{__TAURI_INTERNALS__?: {invoke?: (command: string, args?: unknown) => Promise<SaveOutcome>}, dshDesktop?: {fileActions?: DesktopFileActions}}} BridgeRoot
 */

/**
 * @param {BridgeRoot} root
 * @returns {DesktopFileActions}
 */
export function installFileActionsBridge(root) {
  if (root.dshDesktop?.fileActions) return root.dshDesktop.fileActions
  const invoke = root.__TAURI_INTERNALS__?.invoke
  if (typeof invoke !== 'function') throw new Error('Native file actions are unavailable')

  /** @type {DesktopFileActions} */
  const actions = {
    async save(request) {
      if (!request || typeof request !== 'object' || Array.isArray(request)) {
        throw new Error('Invalid file save request')
      }
      const value = /** @type {Record<string, unknown>} */ (request)
      if (typeof value.url !== 'string' || !value.url.trim()) throw new Error('Invalid file URL')
      let source
      try {
        source = new URL(value.url)
      } catch {
        throw new Error('Invalid file URL')
      }
      if (!['http:', 'https:'].includes(source.protocol) || source.username || source.password || value.url.includes('#')) {
        throw new Error('Unsupported file URL')
      }
      if (value.mode !== 'download' && value.mode !== 'saveAs') throw new Error('Invalid save mode')
      if (value.name !== undefined && typeof value.name !== 'string') throw new Error('Invalid file name')
      if (value.mediaType !== undefined && typeof value.mediaType !== 'string') throw new Error('Invalid media type')

      const clean = {
        url: value.url,
        mode: value.mode,
        ...(value.name === undefined ? {} : { name: value.name }),
        ...(value.mediaType === undefined ? {} : { mediaType: value.mediaType }),
      }
      return invoke('save_download', { request: clean })
    },
    async readClipboardImage() {
      throw new Error('readClipboardImage is unsupported')
    },
  }
  root.dshDesktop = { ...(root.dshDesktop || {}), fileActions: actions }
  return actions
}
