import { resolve } from 'node:path'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'
export default defineConfig({
  plugins: [svelte()],
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
