// @vitest-environment node
import { describe, it, expect } from 'vitest';
import config, { cspDirectives } from '../../svelte.config.js';

/**
 * R15 F4: the dev-server origins stay out of a production CSP, and nothing else in the policy moves.
 * The built meta tag is the final proof; this pins the source it is generated from.
 */
const DEV_ORIGINS = ['http://localhost:*', 'https://localhost:5174'];

function table(isProduction: boolean): Record<string, string[]> {
	return cspDirectives(isProduction) as unknown as Record<string, string[]>;
}

describe('CSP directives (R15 F4)', () => {
	it('omits every localhost origin in production', () => {
		const prod = table(true);

		expect(prod['connect-src']).toEqual(['self']);
		expect(prod['frame-src']).toEqual(['self', 'https://www.openstreetmap.org']);
		for (const values of Object.values(prod)) {
			for (const value of values) expect(value).not.toContain('localhost');
		}
	});

	it('keeps the dev origins outside production, in their original order', () => {
		const dev = table(false);

		expect(dev['connect-src']).toEqual(['self', ...DEV_ORIGINS]);
		expect(dev['frame-src']).toEqual([
			'self',
			'https://localhost:5174',
			'https://www.openstreetmap.org'
		]);
	});

	it('leaves every other directive identical in both modes', () => {
		const prod = table(true);
		const dev = table(false);
		const others = Object.keys(dev).filter((k) => k !== 'connect-src' && k !== 'frame-src');

		expect(Object.keys(prod)).toEqual(Object.keys(dev));
		for (const name of others) expect(prod[name]).toEqual(dev[name]);
		expect(dev['default-src']).toEqual(['self']);
		expect(dev['style-src']).toEqual(['self', 'unsafe-inline']);
		expect(dev['img-src']).toEqual([
			'self',
			'data:',
			'blob:',
			'https://tile.openstreetmap.org',
			'https://*.tile.openstreetmap.org'
		]);
		expect(dev['font-src']).toEqual(['self']);
		expect(dev['object-src']).toEqual(['none']);
		expect(dev['base-uri']).toEqual(['self']);
		expect(dev['form-action']).toEqual(['self']);
	});

	it('keeps script-src hash-only, never unsafe-inline', () => {
		for (const isProduction of [true, false]) {
			const scriptSrc = table(isProduction)['script-src'];
			expect(scriptSrc).toEqual(['self', 'wasm-unsafe-eval']);
			expect(scriptSrc).not.toContain('unsafe-inline');
		}
		expect(config.kit?.csp?.mode).toBe('hash');
	});

	it('wires the config to the NODE_ENV production test', () => {
		expect(config.kit?.csp?.directives).toEqual(
			cspDirectives(process.env.NODE_ENV === 'production')
		);
	});
});
