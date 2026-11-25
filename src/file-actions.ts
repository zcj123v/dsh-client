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
const wrappedRegistrations = new WeakSet()

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
 * Patch one official registration in place: its factory keeps producing the
 * official exports, but the exported `apply` reroutes `fileActions.save`.
 * The official `apply` still runs untouched (menus, icons, locale), and if the
 * official shape drifts (factory no longer returns `{apply}`, or `apply` no
 * longer provides a save-capable `fileActions`), the patch silently steps
 * aside and downloads fall back to official behavior.
 *
 * @param {ModuleRegistration} registration
 * @param {(source: {url: string, name?: string}, mode: string) => unknown} wrapSave
 */
function wrapRegistration(registration, wrapSave) {
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
        // The official `apply` hands the very same object to the context-menu
        // closures, so replacing `save` in place reroutes the real download
        // entry; everything else on the official object stays untouched.
        // Do NOT provide the service a second time — Cordis rejects a
        // duplicate service registration and the whole page boot fails.
        officialActions.save = wrapSave
      }
      return result
    }
    return exports
  }
}

/**
 * Wrap one `load` function so target registrations get patched on the way in.
 * Registrations already patched once pass through unchanged, so a record that
 * the page's module system re-feeds through a later `load` (its `create()`
 * drains the queue through the freshly assigned live loader) is not wrapped
 * twice.
 *
 * @param {(registration: ModuleRegistration) => void} load
 * @param {string} targetId
 * @param {(source: {url: string, name?: string}, mode: string) => unknown} wrapSave
 */
function wrapLoadFunction(load, targetId, wrapSave) {
  return function (registration) {
    if (wrappedRegistrations.has(registration)
      || !registration || typeof registration !== 'object'
      || registration.id !== targetId || typeof registration.factory !== 'function') {
      return load.call(this, registration)
    }
    wrappedRegistrations.add(registration)
    wrapRegistration(registration, wrapSave)
    return load.call(this, registration)
  }
}

/**
 * Wrap a page's module loader object. The `load` property becomes an accessor
 * whose getter returns the wrapped function and whose setter re-wraps whatever
 * function the page assigns later: the module system's `create()` replaces
 * `target.load` wholesale when it switches from queue mode to live mode, and
 * registrations arriving after that must still be patched. Frozen loaders fall
 * back to a plain wrapped `load`, or to official behavior when even that
 * cannot stick.
 *
 * @param {ModuleLoaderTarget} loader
 * @param {string} targetId
 * @param {(source: {url: string, name?: string}, mode: string) => unknown} wrapSave
 */
function wrapModuleLoader(loader, targetId, wrapSave) {
  if (!loader || typeof loader !== 'object' || wrappedLoaders.has(loader)) return
  const load = loader.load
  if (typeof load !== 'function') return
  wrappedLoaders.add(loader)
  const wrapped = wrapLoadFunction(load, targetId, wrapSave)
  let current = wrapped
  try {
    Object.defineProperty(loader, 'load', {
      configurable: true,
      enumerable: true,
      get() { return current },
      set(value) { current = typeof value === 'function' ? wrapLoadFunction(value, targetId, wrapSave) : value },
    })
  } catch {
    try { loader.load = wrapped } catch { /* frozen loader: official behavior stands */ }
  }
}

/**
 * Hook `root.__ModuleLoader__` itself. The initialization script runs at
 * document start, before the page's inline script installs the module loader
 * facade, so a plain read would always miss it: the property becomes an
 * accessor that wraps whatever loader object the page assigns later, before
 * the page's own `load` calls run. A loader that already exists at install
 * time is wrapped directly. The page still owns the property — values are
 * stored and returned verbatim, only their `load` gets wrapped.
 *
 * @param {BridgeRoot} root
 * @param {string} targetId
 * @param {(source: {url: string, name?: string}, mode: string) => unknown} wrapSave
 */
function hookModuleLoaderProperty(root, targetId, wrapSave) {
  if (!root || typeof root !== 'object') return
  const existing = root.__ModuleLoader__
  try {
    let loader = existing
    Object.defineProperty(root, '__ModuleLoader__', {
      configurable: true,
      enumerable: true,
      get() { return loader },
      set(value) {
        loader = value
        if (value && typeof value === 'object') wrapModuleLoader(value, targetId, wrapSave)
      },
    })
  } catch {
    // The property cannot be trapped (frozen or exotic root): wrap what exists.
  }
  if (existing && typeof existing === 'object') wrapModuleLoader(existing, targetId, wrapSave)
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
  hookModuleLoaderProperty(
    root,
    '@deepseek-ai/dsh-client-ui-file-actions',
    (source, mode) => actions.save({ url: source.url, name: source.name, mode }),
  )
  root.dshDesktop = { ...(root.dshDesktop || {}), fileActions: actions }
  return actions
}
