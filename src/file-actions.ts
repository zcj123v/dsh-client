// @ts-nocheck -- Rust executes this JavaScript-compatible .ts source directly.
/**
 * Minimal desktop adapter injected only into macOS node content WebViews.
 * This file intentionally stays valid JavaScript after its module exports are
 * removed, so Rust can initialize the exact tested implementation natively.
 *
 * @typedef {{url: string, name?: string, mediaType?: string, mode: 'download' | 'saveAs'}} SaveRequest
 * @typedef {{status: 'cancelled'} | {status: 'saved', path: string}} SaveOutcome
 * @typedef {{save(request: SaveRequest): Promise<SaveOutcome>, readClipboardImage(url: string): Promise<never>}} DesktopFileActions
 * @typedef {{__TAURI_INTERNALS__?: {invoke?: (command: string, args?: unknown) => Promise<SaveOutcome>}, dshDesktop?: {fileActions?: DesktopFileActions}, document?: {title: string}}} BridgeRoot
 */

let diagnosticTimer
let titleToRestore = ''

function diagnose(root, stage) {
  if (!root.document) return
  if (diagnosticTimer !== undefined) clearTimeout(diagnosticTimer)
  if (!root.document.title.startsWith('[dsh-download] ')) titleToRestore = root.document.title
  const diagnostic = `[dsh-download] ${stage}`
  root.document.title = diagnostic
  diagnosticTimer = setTimeout(() => {
    if (root.document?.title === diagnostic) root.document.title = titleToRestore
    diagnosticTimer = undefined
  }, 50)
}

/**
 * @param {BridgeRoot} root
 * @returns {DesktopFileActions}
 */
export function installFileActionsBridge(root) {
  if (root.dshDesktop?.fileActions) {
    diagnose(root, 'bridge-existing')
    return root.dshDesktop.fileActions
  }
  const invoke = root.__TAURI_INTERNALS__?.invoke
  if (typeof invoke !== 'function') {
    diagnose(root, 'bridge-ipc-unavailable')
    throw new Error('Native file actions are unavailable')
  }
  diagnose(root, 'bridge-install')

  /** @type {DesktopFileActions} */
  const actions = {
    async save(request) {
      diagnose(root, 'save-request')
      if (!request || typeof request !== 'object' || Array.isArray(request)) {
        diagnose(root, 'save-request-rejected')
        throw new Error('Invalid file save request')
      }
      const value = /** @type {Record<string, unknown>} */ (request)
      if (typeof value.url !== 'string' || !value.url.trim()) {
        diagnose(root, 'save-url-rejected')
        throw new Error('Invalid file URL')
      }
      let source
      try {
        source = new URL(value.url)
      } catch {
        diagnose(root, 'save-url-rejected')
        throw new Error('Invalid file URL')
      }
      if (!['http:', 'https:'].includes(source.protocol) || source.username || source.password || value.url.includes('#')) {
        diagnose(root, 'save-url-rejected')
        throw new Error('Unsupported file URL')
      }
      if (value.mode !== 'download' && value.mode !== 'saveAs') {
        diagnose(root, 'save-mode-rejected')
        throw new Error('Invalid save mode')
      }
      if (value.name !== undefined && typeof value.name !== 'string') {
        diagnose(root, 'save-field-rejected')
        throw new Error('Invalid file name')
      }
      if (value.mediaType !== undefined && typeof value.mediaType !== 'string') {
        diagnose(root, 'save-field-rejected')
        throw new Error('Invalid media type')
      }

      const clean = {
        url: value.url,
        mode: value.mode,
        ...(value.name === undefined ? {} : { name: value.name }),
        ...(value.mediaType === undefined ? {} : { mediaType: value.mediaType }),
      }
      diagnose(root, 'save-ipc-start')
      try {
        const outcome = await invoke('save_download', { request: clean })
        if (outcome?.status === 'saved') diagnose(root, 'save-ipc-saved')
        else if (outcome?.status === 'cancelled') diagnose(root, 'save-ipc-cancelled')
        else diagnose(root, 'save-ipc-invalid')
        return outcome
      } catch (error) {
        const message = error instanceof Error ? error.message : typeof error === 'string' ? error : ''
        const aclRejected = /not allowed by ACL|not allowed on (?:origin|window|webview)/i.test(message)
        diagnose(root, aclRejected ? 'save-ipc-acl-rejected' : 'save-ipc-error')
        throw error
      }
    },
    async readClipboardImage() {
      throw new Error('readClipboardImage is unsupported')
    },
  }
  root.dshDesktop = { ...(root.dshDesktop || {}), fileActions: actions }
  return actions
}
