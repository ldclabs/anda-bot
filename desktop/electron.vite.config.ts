import { resolve } from 'node:path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  main: { plugins: [externalizeDepsPlugin()] },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: {
      rollupOptions: { output: { format: 'cjs', entryFileNames: '[name].js' } }
    }
  },
  renderer: {
    root: resolve('src/renderer'),
    resolve: {
      alias: {
        $lib: resolve('../chrome-extension/src/lib'),
        $extension: resolve('../chrome-extension/src')
      },
      dedupe: ['svelte']
    },
    plugins: [tailwindcss(), svelte()],
    build: {
      rollupOptions: { input: resolve('src/renderer/index.html') },
      chunkSizeWarningLimit: 2000
    },
    server: { fs: { allow: [resolve('..')] } }
  }
})
