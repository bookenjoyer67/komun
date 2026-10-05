import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { resolveMapTiles } from './map-tiles.config.js';

const mapTiles = resolveMapTiles(process.env);

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),
	kit: {
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			fallback: 'index.html'
		}),
		csp: {
			mode: 'hash',
			directives: {
				'default-src': ['self'],
				'script-src': ['self', 'wasm-unsafe-eval'],
				'style-src': ['self', 'unsafe-inline'],
				'img-src': [
					'self',
					'data:',
					'blob:',
					// Must come from the same build environment as LocationMap's tile URL
					// (vite.config.ts), or the map's tiles are blocked.
					...mapTiles.cspOrigins
				],
				'frame-src': ['self', 'https://localhost:5174', 'https://www.openstreetmap.org'],
				'connect-src': ['self', 'http://localhost:*', 'https://localhost:5174'],
				'font-src': ['self'],
				'object-src': ['none'],
				'base-uri': ['self'],
				'form-action': ['self']
			}
		}
	}
};

export default config;
