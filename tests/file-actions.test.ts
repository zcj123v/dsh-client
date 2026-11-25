import { afterEach, describe, expect, it, vi } from 'vitest'
import { installFileActionsBridge } from '../src/file-actions'

const url = 'https://node.example:8443/api/file?id=artifact'

function fixture() {
  const invoke = vi.fn().mockResolvedValue({ status: 'saved', path: '/Users/test/Downloads/artifact.apk' })
  const root = { __TAURI_INTERNALS__: { invoke } }
  return { invoke, root }
}

describe('installFileActionsBridge', () => {
  afterEach(() => { vi.useRealTimers() })

  it.each(['download', 'saveAs'] as const)('forwards a %s request with only the native request fields', async mode => {
    const { invoke, root } = fixture()
    const actions = installFileActionsBridge(root)

    const result = await actions.save({
      url,
      name: 'artifact.apk',
      mediaType: 'application/vnd.android.package-archive',
      mode,
      token: 'TOKEN_SENTINEL',
      cookie: 'COOKIE_SENTINEL',
      path: '/private/choice.apk',
      read: () => Promise.reject(new Error('must not read through the page')),
    })

    expect(result).toEqual({ status: 'saved', path: '/Users/test/Downloads/artifact.apk' })
    expect(invoke).toHaveBeenCalledOnce()
    expect(invoke).toHaveBeenCalledWith('save_download', {
      request: { url, name: 'artifact.apk', mediaType: 'application/vnd.android.package-archive', mode },
    })
    const serialized = JSON.stringify(invoke.mock.calls[0])
    expect(serialized).not.toContain('TOKEN_SENTINEL')
    expect(serialized).not.toContain('COOKIE_SENTINEL')
    expect(serialized).not.toContain('/private/choice.apk')
    expect(serialized).not.toContain('read')
  })

  it('omits optional fields instead of forwarding undefined or unknown values', async () => {
    const { invoke, root } = fixture()
    const actions = installFileActionsBridge(root)

    await actions.save({ url, mode: 'download', token: 'never-forward', path: '/tmp/never' })

    expect(invoke).toHaveBeenCalledWith('save_download', { request: { url, mode: 'download' } })
  })

  it.each([
    [{ url: '', mode: 'download' }, 'empty URL'],
    [{ url: '   ', mode: 'download' }, 'blank URL'],
    [{ url: '/api/file', mode: 'download' }, 'relative URL'],
    [{ url: 'file:///tmp/artifact.apk', mode: 'download' }, 'file URL'],
    [{ url: 'blob:https://node.example/id', mode: 'download' }, 'blob URL'],
    [{ url: `${url}#access_token=FRAGMENT_SENTINEL`, mode: 'download' }, 'fragment credentials'],
    [{ url: `${url}#`, mode: 'download' }, 'empty fragment'],
    [{ url, mode: 'delete' }, 'unknown mode'],
    [{ url, mode: 'download', name: 42 }, 'non-string name'],
    [{ url, mode: 'download', mediaType: 42 }, 'non-string media type'],
  ])('rejects %s before native IPC', async request => {
    const { invoke, root } = fixture()
    const actions = installFileActionsBridge(root)

    await expect(actions.save(request)).rejects.toThrow()
    expect(invoke).not.toHaveBeenCalled()
  })

  it('does not replace an installed bridge when initialization runs again', () => {
    const { invoke, root } = fixture()
    const first = installFileActionsBridge(root)
    const second = installFileActionsBridge(root)

    expect(second).toBe(first)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('keeps clipboard image reads explicitly unsupported', async () => {
    const { invoke, root } = fixture()
    const actions = installFileActionsBridge(root)

    await expect(actions.readClipboardImage(url)).rejects.toThrow(/unsupported/i)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('emits fixed stage titles and asynchronously restores the original title without sensitive values', async () => {
    vi.useFakeTimers()
    const titles: string[] = []
    let title = 'ORIGINAL_TITLE_SENTINEL'
    const document = {
      get title() { return title },
      set title(value: string) { titles.push(value); title = value },
    }
    const invoke = vi.fn().mockResolvedValue({ status: 'saved', path: '/Users/test/Downloads/private.apk' })
    const root = { __TAURI_INTERNALS__: { invoke }, document }
    const actions = installFileActionsBridge(root)

    await actions.save({ url: `${url}&token=URL_SENTINEL`, name: 'artifact.apk', mode: 'download', cookie: 'COOKIE_SENTINEL' })

    const stages = titles.filter(value => value.startsWith('[dsh-download] '))
    expect(stages).toEqual([
      '[dsh-download] bridge-install',
      '[dsh-download] save-request',
      '[dsh-download] save-ipc-start',
      '[dsh-download] save-ipc-saved',
    ])
    expect(document.title).toBe('[dsh-download] save-ipc-saved')
    expect(titles).toHaveLength(stages.length)
    await vi.advanceTimersByTimeAsync(50)
    expect(document.title).toBe('ORIGINAL_TITLE_SENTINEL')
    expect(titles).toHaveLength(stages.length + 1)
    expect(titles.at(-1)).toBe('ORIGINAL_TITLE_SENTINEL')
    const output = JSON.stringify(stages)
    for (const sentinel of ['URL_SENTINEL', 'COOKIE_SENTINEL', url, '/Users/test/Downloads/private.apk', 'ORIGINAL_TITLE_SENTINEL']) {
      expect(output).not.toContain(sentinel)
    }
  })

  it('distinguishes rejected URLs and remote IPC rejection without leaking error text', async () => {
    vi.useFakeTimers()
    const titles: string[] = []
    let title = 'original'
    const document = {
      get title() { return title },
      set title(value: string) { titles.push(value); title = value },
    }
    const invoke = vi.fn()
      .mockRejectedValueOnce(new Error('Command not allowed by ACL; URL_SENTINEL COOKIE_SENTINEL'))
    const root = { __TAURI_INTERNALS__: { invoke }, document }
    const actions = installFileActionsBridge(root)

    await expect(actions.save({ url: 'file:///tmp/SECRET_SENTINEL', mode: 'download' })).rejects.toThrow()
    expect(titles.filter(value => value.startsWith('[dsh-download] '))).toContain('[dsh-download] save-url-rejected')
    expect(invoke).not.toHaveBeenCalled()

    await expect(actions.save({ url, mode: 'download' })).rejects.toThrow()
    expect(titles.filter(value => value.startsWith('[dsh-download] ')).at(-1)).toBe('[dsh-download] save-ipc-acl-rejected')
    expect(document.title).toBe('[dsh-download] save-ipc-acl-rejected')
    await vi.advanceTimersByTimeAsync(50)
    expect(document.title).toBe('original')
    const output = JSON.stringify(titles.filter(value => value.startsWith('[dsh-download] ')))
    expect(output).not.toContain('URL_SENTINEL')
    expect(output).not.toContain('COOKIE_SENTINEL')
    expect(output).not.toContain('SECRET_SENTINEL')
    expect(output).not.toContain('Command not allowed')
  })

  it('reports an initialization-time missing native IPC bridge through an asynchronously restored title', () => {
    vi.useFakeTimers()
    const titles: string[] = []
    let title = 'original'
    const document = {
      get title() { return title },
      set title(value: string) { titles.push(value); title = value },
    }

    expect(() => installFileActionsBridge({ document })).toThrow('Native file actions are unavailable')
    expect(titles).toEqual(['[dsh-download] bridge-ipc-unavailable'])
    expect(document.title).toBe('[dsh-download] bridge-ipc-unavailable')
    vi.advanceTimersByTime(50)
    expect(titles).toEqual(['[dsh-download] bridge-ipc-unavailable', 'original'])
    expect(document.title).toBe('original')
  })
})

describe('official ui-file-actions module hook', () => {
  it('wraps the official file-actions factory so save skips source.read', async () => {
    const { invoke, root } = fixture()
    const registered = new Map<string, { id: string; factory: (require: unknown) => unknown }>()
    const loader = { load(record: { id: string; factory: unknown }) { registered.set(record.id, record as never) } }
    Object.assign(root, { __ModuleLoader__: loader })
    installFileActionsBridge(root)

    // 模拟官方包：apply 时 provide 一个会预读的 actions
    const officialRead = vi.fn(async () => { throw new Error('too-large') })
    const officialFactory = () => ({
      inject: [],
      apply(ctx: { provide: (n: string, v: unknown) => void }) {
        ctx.provide('fileActions', { canSaveAs: true, save: async () => ({ status: 'saved' }), copyImage: async () => {} })
      },
    })
    loader.load({ id: '@deepseek-ai/dsh-client-ui-file-actions', factory: officialFactory })
    const provided: Record<string, unknown> = {}
    const ctx = { provide: (n: string, v: unknown) => { provided[n] = v } }
    // 官方模块协议：factory(require) 返回 exports，之后才调 exports.apply(ctx)
    // （对照真实官方 bundle 与 dsh-client-modules 的 materialize 实现）。
    const officialExports = registered.get('@deepseek-ai/dsh-client-ui-file-actions')!.factory({}) as {
      apply: (ctx: { provide: (n: string, v: unknown) => void }) => void
    }
    officialExports.apply(ctx)
    const actions = provided.fileActions as { save: (s: unknown, m: string) => Promise<unknown>; canSaveAs: boolean }

    const source = { url: 'http://127.0.0.1:40286/api/file?path=/x.apk', name: 'x.apk', read: officialRead }
    await actions.save(source, 'download')
    expect(officialRead).not.toHaveBeenCalled()
    expect(invoke).toHaveBeenCalledWith('save_download', { request: { url: source.url, name: 'x.apk', mode: 'download' } })
    expect(actions.canSaveAs).toBe(true)
  })

  it('leaves unrelated module ids untouched', () => {
    const { root } = fixture()
    const registered = new Map<string, { id: string; factory: unknown }>()
    const loader = { load(r: { id: string; factory: unknown }) { registered.set(r.id, r) } }
    installFileActionsBridge(Object.assign(root, { __ModuleLoader__: loader }))
    const factory = () => ({ apply() {} })
    loader.load({ id: 'other-plugin', factory })
    expect(registered.get('other-plugin')!.factory).toBe(factory)
  })

  it('wraps a shared loader only once across repeated installations', () => {
    const { root } = fixture()
    const loader = { load(_r: { id: string; factory: unknown }) {} }
    Object.assign(root, { __ModuleLoader__: loader })
    installFileActionsBridge(root)
    const wrappedLoad = loader.load
    const second = fixture()
    Object.assign(second.root, { __ModuleLoader__: loader })
    installFileActionsBridge(second.root)
    expect(loader.load).toBe(wrappedLoad)
  })
})
