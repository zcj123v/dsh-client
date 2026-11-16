import { describe, expect, it } from 'vitest'
import { connectionCapabilities } from '../src/platform-capabilities'

describe('platform connection capabilities', () => {
  it('offers SSH and loopback only on desktop', () => {
    expect(connectionCapabilities('Mozilla/5.0 (Macintosh)')).toEqual({ android: false, sshForward: true, loopback: true })
    expect(connectionCapabilities('Mozilla/5.0 (Windows NT 10.0)')).toEqual({ android: false, sshForward: true, loopback: true })
  })
  it('requires HTTPS and native isolated profiles on Android', () => {
    expect(connectionCapabilities('Mozilla/5.0 (Linux; Android 15; Pixel)')).toEqual({ android: true, sshForward: false, loopback: false })
  })
})
