import type { ConnectionRecord } from './connection-model'

export function webviewIsolationOptions(record: ConnectionRecord): { dataDirectory?: string; dataStoreIdentifier?: number[] } {
  const bytes = new TextEncoder().encode(record.partition.padEnd(16, '0').slice(0, 16))
  return {
    dataDirectory: record.partition,
    dataStoreIdentifier: Array.from(bytes),
  }
}
