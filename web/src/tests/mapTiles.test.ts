import { describe, it, expect } from 'vitest';
import { resolveMapTiles } from '../../map-tiles.config.js';

const OSM_TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
const OSM_ORIGINS = ['https://tile.openstreetmap.org', 'https://*.tile.openstreetmap.org'];

/**
 * Read at build time, because the CSP is a build-time meta tag: a value served at runtime could
 * change the tile URL but never the `img-src` that has to allow it.
 */
describe('map tile configuration', () => {
	it('defaults to OpenStreetMap when nothing is set', () => {
		expect(resolveMapTiles({})).toEqual({ tileUrl: OSM_TILE_URL, cspOrigins: OSM_ORIGINS });
	});

	it('treats a blank value as unset', () => {
		expect(resolveMapTiles({ KOMUN_TILE_URL: '  ', KOMUN_TILE_CSP_ORIGINS: '' })).toEqual({
			tileUrl: OSM_TILE_URL,
			cspOrigins: OSM_ORIGINS
		});
	});

	it("uses the operator's tile URL and CSP origins when set", () => {
		const resolved = resolveMapTiles({
			KOMUN_TILE_URL: 'https://tiles.example.org/{z}/{x}/{y}.png',
			KOMUN_TILE_CSP_ORIGINS: 'https://tiles.example.org, https://*.tiles.example.org'
		});

		expect(resolved).toEqual({
			tileUrl: 'https://tiles.example.org/{z}/{x}/{y}.png',
			cspOrigins: ['https://tiles.example.org', 'https://*.tiles.example.org']
		});
	});
});
