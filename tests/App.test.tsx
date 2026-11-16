// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StrictMode } from 'react'

afterEach(cleanup)
it('keeps the token input usable after React releases the input event and never saves it', async () => {
  localStorage.setItem('dsh-client.connections.v1', JSON.stringify([{ id:'170ce412-3cbb-4467-a35a-b49fce65e8ef', name:'Test', mode:'direct', url:'http://127.0.0.1:3080/', remotePort:3080, partition:'170ce412-3cbb-4467-a35a-b49fce65e8ef', createdAt:'2026-11-16T00:00:00Z' }]))
  vi.resetModules()
  const { default: App } = await import('../src/App')
  render(<StrictMode><App /></StrictMode>)
  const input = screen.getByLabelText('Test 一次性 token') as HTMLInputElement
  fireEvent.change(input, { target: { value: 'TEST_SENTINEL' } })
  fireEvent.change(input, { target: { value: 'TEST_SENTINEL_TWO' } })
  expect(input.value).toBe('TEST_SENTINEL_TWO')
  expect(localStorage.getItem('dsh-client.connections.v1')).not.toContain('SENTINEL')
})
