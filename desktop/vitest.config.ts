import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'
export default defineConfig({
  resolve: {
    alias: {
      // Renderer helpers import the shared extension modules the app bundles.
      $lib: resolve('../chrome-extension/src/lib')
    }
  },
  test: {
    include: ['tests/**/*.test.ts'],
    environment: 'node',
    testTimeout: 15_000
  }
})
