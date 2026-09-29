import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import type { PluginOption } from 'vite'
import { defineConfig } from 'vite'

// KaTeX's stylesheet lists woff2, woff and ttf sources for every font. Chrome
// 116+ always takes the first (woff2), so the fallbacks are dropped from the
// CSS and the package instead of shipping about 0.9 MB that is never loaded.
const katexFallbackFontSource =
  /,\s*url\((?:[^)]*KaTeX_[\w-]+\.(?:woff|ttf)|data:font\/(?:woff|ttf)[^)]*)\)\s*format\(["'](?:woff|truetype)["']\)/g

function katexWoff2Only(): PluginOption {
  return {
    name: 'anda-katex-woff2-only',
    apply: 'build',
    enforce: 'post',
    generateBundle(_options, bundle) {
      for (const [fileName, output] of Object.entries(bundle)) {
        if (/KaTeX_[\w-]+\.(?:woff|ttf)$/.test(fileName)) {
          delete bundle[fileName]
        } else if (
          output.type === 'asset' &&
          fileName.endsWith('.css') &&
          typeof output.source === 'string'
        ) {
          output.source = output.source.replace(katexFallbackFontSource, '')
        }
      }
    }
  }
}

const plugins: PluginOption[] = [
  tailwindcss() as PluginOption,
  svelte() as PluginOption,
  katexWoff2Only()
]

export default defineConfig({
  base: './',
  plugins,
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        index: path.resolve('index.html'),
        dashboard: path.resolve('dashboard.html'),
        service_worker: path.resolve('src/service_worker.ts'),
        page_element_content: path.resolve('src/page_element_content.ts')
      },
      output: {
        entryFileNames: (chunkInfo) =>
          chunkInfo.name === 'service_worker' ? 'service_worker.js' : 'assets/[name].js',
        chunkFileNames: `assets/[name].js`,
        assetFileNames: `assets/[name].[ext]`
      }
    }
  },
  resolve: {
    alias: {
      $lib: path.resolve('./src/lib')
    }
  }
})
