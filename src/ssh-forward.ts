export interface SshForwardInput {
  profile: string
  user?: string
  remotePort: number
  localPort: number
}

const SAFE_SSH_PART = /^[A-Za-z0-9._:@/-]+$/

export function buildSshForwardArgs(input: SshForwardInput): string[] {
  if (!SAFE_SSH_PART.test(input.profile) || (input.user !== undefined && !SAFE_SSH_PART.test(input.user))) {
    throw new Error('SSH Host 别名或用户包含不允许的字符')
  }
  if (!Number.isInteger(input.remotePort) || input.remotePort < 1 || input.remotePort > 65535) throw new Error('SSH 远端端口无效')
  if (!Number.isInteger(input.localPort) || input.localPort < 1 || input.localPort > 65535) throw new Error('SSH 本地端口无效')
  const destination = input.user ? `${input.user}@${input.profile}` : input.profile
  return [
    '-N', '-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3',
    '-L', `127.0.0.1:${input.localPort}:127.0.0.1:${input.remotePort}`, destination,
  ]
}
