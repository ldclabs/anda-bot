import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'
import type { PluginOption } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import path from 'path'

const plugins: PluginOption[] = [tailwindcss() as PluginOption, svelte() as PluginOption]

export default defineConfig({
	base: './',
	plugins,
	build: {
		outDir: 'dist',
		rollupOptions: {
			input: {
				index: path.resolve('index.html'),
				service_worker: path.resolve('src/service_worker.ts')
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
