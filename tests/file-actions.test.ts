import { describe, expect, it, vi } from 'vitest'
import { installFileActionsBridge } from '../src/file-actions'

const url = 'https://node.example:8443/api/file?id=artifact'

function fixture() {
  const invoke = vi.fn().mockResolvedValue({ status: 'saved', path: '/Users/test/Downloads/artifact.apk' })
  const root = { __TAURI_INTERNALS__: { invoke } }
  return { invoke, root }
}

describe('installFileActionsBridge', () => {
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

  it('emits fixed stage diagnostics without URL, cookie, token, path or native error text', async () => {
    const info = vi.fn()
    const invoke = vi.fn().mockResolvedValue({ status: 'saved', path: '/Users/test/Downloads/private.apk' })
    const root = { __TAURI_INTERNALS__: { invoke }, console: { info } }
    const actions = installFileActionsBridge(root)

    await actions.save({ url: `${url}&token=URL_SENTINEL`, name: 'artifact.apk', mode: 'download', cookie: 'COOKIE_SENTINEL' })

    expect(info.mock.calls.map(call => call[0])).toEqual([
      '[dsh-download] bridge-install',
      '[dsh-download] save-request',
      '[dsh-download] save-ipc-start',
      '[dsh-download] save-ipc-saved',
    ])
    const output = JSON.stringify(info.mock.calls)
    for (const sentinel of ['URL_SENTINEL', 'COOKIE_SENTINEL', url, '/Users/test/Downloads/private.apk']) {
      expect(output).not.toContain(sentinel)
    }
  })

  it('distinguishes rejected URLs and remote IPC rejection without leaking error text', async () => {
    const info = vi.fn()
    const invoke = vi.fn()
      .mockRejectedValueOnce(new Error('Command not allowed by ACL; URL_SENTINEL COOKIE_SENTINEL'))
    const root = { __TAURI_INTERNALS__: { invoke }, console: { info } }
    const actions = installFileActionsBridge(root)

    await expect(actions.save({ url: 'file:///tmp/SECRET_SENTINEL', mode: 'download' })).rejects.toThrow()
    expect(info).toHaveBeenLastCalledWith('[dsh-download] save-url-rejected')
    expect(invoke).not.toHaveBeenCalled()

    await expect(actions.save({ url, mode: 'download' })).rejects.toThrow()
    expect(info).toHaveBeenLastCalledWith('[dsh-download] save-ipc-acl-rejected')
    const output = JSON.stringify(info.mock.calls)
    expect(output).not.toContain('URL_SENTINEL')
    expect(output).not.toContain('COOKIE_SENTINEL')
    expect(output).not.toContain('SECRET_SENTINEL')
    expect(output).not.toContain('Command not allowed')
  })

  it('reports an initialization-time missing native IPC bridge', () => {
    const info = vi.fn()

    expect(() => installFileActionsBridge({ console: { info } })).toThrow('Native file actions are unavailable')
    expect(info).toHaveBeenCalledWith('[dsh-download] bridge-ipc-unavailable')
  })
})
