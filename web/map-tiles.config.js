// Read at build time, because the CSP is a build-time meta tag: a value served at runtime could move
// the tile URL but never the `img-src` that has to allow it.

export const OSM_TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';

export const OSM_TILE_CSP_ORIGINS = [
	'https://tile.openstreetmap.org',
	'https://*.tile.openstreetmap.org'
];

/**
 * The origins are a separate setting because a URL template such as `https://{s}.tiles.example/`
 * cannot be parsed into the origin the CSP needs. A blank value counts as unset.
 *
 * @param {Record<string, string | undefined>} env
 * @returns {{ tileUrl: string, cspOrigins: string[] }}
 */
export function resolveMapTiles(env) {
	const tileUrl = env.KOMUN_TILE_URL?.trim() || OSM_TILE_URL;
	const listed = (env.KOMUN_TILE_CSP_ORIGINS ?? '')
		.split(',')
		.map((origin) => origin.trim())
		.filter((origin) => origin !== '');
	return { tileUrl, cspOrigins: listed.length > 0 ? listed : [...OSM_TILE_CSP_ORIGINS] };
}
