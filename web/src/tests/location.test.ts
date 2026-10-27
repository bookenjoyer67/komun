import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { get } from 'svelte/store';
import { geocode, clearLocation, location } from '$lib/stores/location';
import { getActiveServer } from '$lib/stores/server';

vi.mock('$lib/stores/server', () => ({
	getActiveServer: vi.fn(() => null),
	isServingOrigin: vi.fn(() => true)
}));

// A directory is someone else's server: what the user typed must not reach it.
vi.mock('$lib/stores/directories', () => ({
	getDirectories: vi.fn(() => ['https://directory.test'])
}));

type FetchMock = Mock<(input: RequestInfo | URL) => Promise<Response>>;

const hit = { lat: '37.8044', lon: '-122.2712', display_name: 'Oakland, California, United States' };

const stLouis = {
	lat: '38.6270',
	lon: '-90.1994',
	display_name: '63101, Saint Louis, Missouri, United States'
};

describe('geocode', () => {
	let fetchMock: FetchMock;

	function respondWith(body: object) {
		fetchMock.mockImplementation(
			async (_input: RequestInfo | URL) => new Response(JSON.stringify(body), { status: 200 })
		);
	}

	beforeEach(() => {
		clearLocation();
		fetchMock = vi.fn(
			async (_input: RequestInfo | URL) => new Response(JSON.stringify(hit), { status: 200 })
		);
		vi.stubGlobal('fetch', fetchMock);
	});

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('sends nothing anywhere when no server is active', async () => {
		vi.mocked(getActiveServer).mockReturnValue(null);

		expect(await geocode('Oakland')).toBe(false);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('asks only the active server', async () => {
		vi.mocked(getActiveServer).mockReturnValue('https://active.test');

		expect(await geocode('Oakland')).toBe(true);
		expect(fetchMock).toHaveBeenCalledTimes(1);
		const url = new URL(String(fetchMock.mock.calls[0][0]));
		expect(url.origin).toBe('https://active.test');
		expect(url.pathname).toBe('/api/geocode');
	});

	it('labels the chip "<city>, <state>" when the server sends both', async () => {
		vi.mocked(getActiveServer).mockReturnValue('https://active.test');
		respondWith({ ...stLouis, city: 'St. Louis', state: 'MO' });

		expect(await geocode('63101')).toBe(true);
		expect(get(location).name).toBe('St. Louis, MO');
	});

	it('falls back to the display_name slice when city or state is missing', async () => {
		vi.mocked(getActiveServer).mockReturnValue('https://active.test');

		for (const parts of [
			{},
			{ city: 'St. Louis' },
			{ state: 'MO' },
			{ city: 'St. Louis', state: null },
			{ city: '', state: 'MO' },
			{ city: 'St. Louis', state: '   ' }
		]) {
			clearLocation();
			respondWith({ ...stLouis, ...parts });

			expect(await geocode('63101')).toBe(true);
			expect(get(location).name, JSON.stringify(parts)).toBe('63101, Saint Louis');
		}
	});

	it('a non-string city falls back', async () => {
		vi.mocked(getActiveServer).mockReturnValue('https://active.test');
		respondWith({ ...stLouis, city: 42, state: 'MO' });

		expect(await geocode('63101')).toBe(true);
		expect(get(location).name).toBe('63101, Saint Louis');
	});
});
