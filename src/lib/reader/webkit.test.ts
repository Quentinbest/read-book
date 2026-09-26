import { describe, expect, it } from 'vitest'
import { readerEngineSupported } from './webkit'

describe('D7-WebKit', () => {
  it('a current engine supports regex lookbehind', () => {
    expect(readerEngineSupported()).toBe(true)
  })
})
