import { describe, it, expect, vi, afterEach } from 'vitest';

const ORIGIN = window.location.origin;
const LEGACY_KEY = 'komun_directories';

/** The store reads storage and the build setting once, at import, so each case needs a fresh load. */
async function loadDirectories() {
	vi.resetModules();
	return import('$lib/stores/directories');
}

function directoryKeys(): string[] {
	const keys: string[] = [];
	for (let i = 0; i < localStorage.length; i++) {
		const key = localStorage.key(i);
		if (key !== null && /director/i.test(key)) keys.push(key);
	}
	return keys;
}

afterEach(() => {
	vi.unstubAllGlobals();
	vi.unstubAllEnvs();
	localStorage.clear();
});

describe('default directory', () => {
	it('R1: with no directory configured, the directory is the origin that served the app', async () => {
		vi.stubEnv('VITE_DEFAULT_DIRECTORY', '');

		const { getDirectories } = await loadDirectories();

		expect(getDirectories()).toEqual([ORIGIN]);
	});

	it('R2: a previously stored directory list is ignored and removed', async () => {
		vi.stubEnv('VITE_DEFAULT_DIRECTORY', '');
		localStorage.setItem(LEGACY_KEY, JSON.stringify(['https://unreachable.example']));

		const { getDirectories } = await loadDirectories();

		expect(getDirectories()).toEqual([ORIGIN]);
		expect(localStorage.getItem(LEGACY_KEY)).toBeNull();
	});

	it('R3: loading the directories writes nothing to storage', async () => {
		vi.stubEnv('VITE_DEFAULT_DIRECTORY', '');

		await loadDirectories();

		expect(directoryKeys()).toEqual([]);
	});

	it('R4: a configured directory is used as given', async () => {
		vi.stubEnv('VITE_DEFAULT_DIRECTORY', 'https://dir.example');

		const { getDirectories } = await loadDirectories();

		expect(getDirectories()).toEqual(['https://dir.example']);
	});
});
