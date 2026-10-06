import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { resolveMapTiles } from './map-tiles.config.js';

const mapTiles = resolveMapTiles(process.env);

/** @typedef {NonNullable<NonNullable<import('@sveltejs/kit').KitConfig['csp']>['directives']>} CspDirectiveSet */
/** @typedef {NonNullable<CspDirectiveSet['connect-src']>} CspSourceList */

/**
 * The localhost origins serve the dev server only; a production page that kept them could reach a
 * visitor's local services and frame a dev origin.
 * @param {boolean} isProduction
 * @returns {CspDirectiveSet}
 */
export function cspDirectives(isProduction) {
	/** @type {CspSourceList} */
	const devFrameSources = isProduction ? [] : ['https://localhost:5174'];
	/** @type {CspSourceList} */
	const devConnectSources = isProduction ? [] : ['http://localhost:*', 'https://localhost:5174'];

	return {
		'default-src': ['self'],
		'script-src': ['self', 'wasm-unsafe-eval'],
		'style-src': ['self', 'unsafe-inline'],
		'img-src': [
			'self',
			'data:',
			'blob:',
			// Map tiles (LocationMap / /map). An operator pointing
			// [map] tile_url at another tile host must add it here too.
			'https://tile.openstreetmap.org',
			'https://*.tile.openstreetmap.org'
		],
		'frame-src': ['self', ...devFrameSources, 'https://www.openstreetmap.org'],
		'connect-src': ['self', ...devConnectSources],
		'font-src': ['self'],
		'object-src': ['none'],
		'base-uri': ['self'],
		'form-action': ['self']
	};
}

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
			// A production build must run with NODE_ENV=production (vite build sets it when unset);
			// any other value ships the dev origins.
			directives: cspDirectives(process.env.NODE_ENV === 'production')
		}
	}
};

export default config;
