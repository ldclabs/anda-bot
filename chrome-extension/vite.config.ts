import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import type { PluginOption } from 'vite'
import { defineConfig } from 'vite'

const plugins: PluginOption[] = [tailwindcss() as PluginOption, svelte() as PluginOption]

export default defineConfig({
	base: './',
	plugins,
	build: {
		outDir: 'dist',
		chunkSizeWarningLimit: 1000,
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
