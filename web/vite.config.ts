import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import wasm from 'vite-plugin-wasm';
import topLevelAwait from 'vite-plugin-top-level-await';
import { resolveMapTiles } from './map-tiles.config.js';

export default defineConfig({
	plugins: [sveltekit(), wasm(), topLevelAwait()],
	define: {
		__KOMUN_TILE_URL__: JSON.stringify(resolveMapTiles(process.env).tileUrl)
	},
	server: {
		allowedHosts: ['komun.buzz', 'localhost'],
		proxy: {
			'/api': 'http://localhost:3000',
			'/avatars': 'http://localhost:3000',
			'/post-images': 'http://localhost:3000',
		},
		fs: {
			allow: ['..']
		}
	}
});
