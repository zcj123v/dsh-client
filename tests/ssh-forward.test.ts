import { describe, expect, it } from 'vitest'
import { buildSshForwardArgs } from '../src/ssh-forward'

describe('SSH forward arguments', () => {
  it('uses fixed local loopback forwarding without shell interpolation', () => {
    expect(buildSshForwardArgs({ profile: 'dsh-mac', user: 'zcj', remotePort: 3080, localPort: 43123 })).toEqual([
      '-N', '-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3',
      '-L', '127.0.0.1:43123:127.0.0.1:3080', 'zcj@dsh-mac',
    ])
  })

  it('rejects shell metacharacters in SSH profile and user', () => {
    expect(() => buildSshForwardArgs({ profile: 'dsh;rm', remotePort: 3080, localPort: 43123 })).toThrow('SSH')
    expect(() => buildSshForwardArgs({ profile: 'dsh-mac', user: 'zcj && rm', remotePort: 3080, localPort: 43123 })).toThrow('SSH')
  })
})
