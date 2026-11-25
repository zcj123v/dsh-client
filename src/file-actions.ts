// @ts-nocheck -- Rust executes this JavaScript-compatible .ts source directly.
/**
 * Minimal desktop adapter injected only into macOS node content WebViews.
 * This file intentionally stays valid JavaScript after its module exports are
 * removed, so Rust can initialize the exact tested implementation natively.
 * Because that strip only covers the single `export function
 * installFileActionsBridge` prefix, every helper below stays a plain
 * module-level function with no `export` of its own.
 *
 * @typedef {{url: string, name?: string, mediaType?: string, mode: 'download' | 'saveAs'}} SaveRequest
 * @typedef {{status: 'cancelled'} | {status: 'saved', path: string}} SaveOutcome
 * @typedef {{save(request: SaveRequest): Promise<SaveOutcome>, readClipboardImage(url: string): Promise<never>}} DesktopFileActions
 * @typedef {{id: string, factory?: unknown}} ModuleRegistration
 * @typedef {{load(registration: ModuleRegistration): void}} ModuleLoaderTarget
 * @typedef {{apply?: (ctx: {provide?: (name: string, value: unknown) => void}) => unknown}} ModuleExports
 * @typedef {{__TAURI_INTERNALS__?: {invoke?: (command: string, args?: unknown) => Promise<SaveOutcome>}, __ModuleLoader__?: ModuleLoaderTarget, dshDesktop?: {fileActions?: DesktopFileActions}, document?: {title: string}}} BridgeRoot
 */

let diagnosticTimer
let titleToRestore = ''
const wrappedLoaders = new WeakSet()

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
 * Wrap a page's `__ModuleLoader__` so the official
 * `@deepseek-ai/dsh-client-ui-file-actions` registration grows a patched
 * `fileActions`: `save` forwards the original source URL straight to the
 * native bridge instead of letting the official implementation pre-read the
 * file into a Blob first. The official `apply` still runs untouched (menus,
 * icons, locale), any other module id passes through unchanged, and a loader
 * already wrapped once is left alone so re-injection stays idempotent. If the
 * official shape drifts (factory no longer returns `{apply}`, or `apply` no
 * longer provides a save-capable `fileActions`), the patch silently steps
 * aside and downloads fall back to official behavior.
 *
 * @param {ModuleLoaderTarget | undefined} loader
 * @param {string} targetId
 * @param {(source: {url: string, name?: string}, mode: string) => unknown} wrapSave
 */
function wrapModuleLoader(loader, targetId, wrapSave) {
  if (!loader || typeof loader.load !== 'function' || wrappedLoaders.has(loader)) return
  wrappedLoaders.add(loader)
  const originalLoad = loader.load
  loader.load = function (registration) {
    if (!registration || typeof registration !== 'object'
      || registration.id !== targetId || typeof registration.factory !== 'function') {
      return originalLoad.call(this, registration)
    }
    const officialFactory = registration.factory
    registration.factory = function (require) {
      const exports = officialFactory.call(this, require)
      if (!exports || typeof exports !== 'object' || typeof exports.apply !== 'function') return exports
      const officialApply = exports.apply
      exports.apply = function (ctx) {
        const officialProvide = ctx && typeof ctx === 'object' && typeof ctx.provide === 'function' ? ctx.provide : undefined
        if (officialProvide === undefined) return officialApply.call(this, ctx)
        let officialActions = null
        ctx.provide = function (name, value) {
          if (name === 'fileActions') officialActions = value
          return officialProvide.call(this, name, value)
        }
        const result = officialApply.call(this, ctx)
        if (officialActions && typeof officialActions === 'object' && typeof officialActions.save === 'function') {
          const patched = Object.create(officialActions)
          patched.save = wrapSave
          officialProvide.call(ctx, 'fileActions', patched)
        }
        return result
      }
      return exports
    }
    return originalLoad.call(this, registration)
  }
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
  wrapModuleLoader(
    root.__ModuleLoader__,
    '@deepseek-ai/dsh-client-ui-file-actions',
    (source, mode) => actions.save({ url: source.url, name: source.name, mode }),
  )
  root.dshDesktop = { ...(root.dshDesktop || {}), fileActions: actions }
  return actions
}
