import { describe, it, expect, vi, afterEach } from 'vitest';

const ORIGIN = window.location.origin;
const STORAGE_KEY = 'komun_servers';
const KNOWN = { url: 'https://kept.example', name: 'Kept', description: '', lastSeen: 0 };

/** The store reads storage and the build setting once, at import, so each case needs a fresh load. */
async function loadServerStore() {
	vi.resetModules();
	return import('$lib/stores/server');
}

function store(state: unknown) {
	localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

afterEach(() => {
	vi.unstubAllGlobals();
	vi.unstubAllEnvs();
	localStorage.clear();
});

describe('default active server', () => {
	it('D1: with nothing stored, the active server is the origin that served the app', async () => {
		vi.stubEnv('VITE_DEFAULT_SERVER', '');

		const { getActiveServer } = await loadServerStore();

		expect(getActiveServer()).toBe(ORIGIN);
	});

	it('D2: a stored empty choice falls back to the origin that served the app', async () => {
		vi.stubEnv('VITE_DEFAULT_SERVER', '');
		store({ active: null, known: [] });

		const { getActiveServer } = await loadServerStore();

		expect(getActiveServer()).toBe(ORIGIN);
	});

	it('D3: a visitor who has servers but none active keeps no active server', async () => {
		vi.stubEnv('VITE_DEFAULT_SERVER', '');
		store({ active: null, known: [KNOWN] });

		const { getActiveServer } = await loadServerStore();

		expect(getActiveServer()).toBeNull();
	});

	it('D4: a build that opts out of a default leaves the choice empty', async () => {
		vi.stubEnv('VITE_DEFAULT_SERVER', 'none');

		const { getActiveServer } = await loadServerStore();

		expect(getActiveServer()).toBeNull();
	});

	it('D5: a stored active server is kept', async () => {
		vi.stubEnv('VITE_DEFAULT_SERVER', '');
		store({ active: KNOWN.url, known: [KNOWN] });

		const { getActiveServer } = await loadServerStore();

		expect(getActiveServer()).toBe(KNOWN.url);
	});
});
