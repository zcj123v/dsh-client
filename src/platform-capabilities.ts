export function connectionCapabilities(userAgent: string): { android: boolean; sshForward: boolean; loopback: boolean } {
  const android = /Android/i.test(userAgent)
  return { android, sshForward: !android, loopback: !android }
}
