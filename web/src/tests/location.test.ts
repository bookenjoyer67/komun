import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { geocode, clearLocation } from '$lib/stores/location';
import { getActiveServer } from '$lib/stores/server';

vi.mock('$lib/stores/server', () => ({
	getActiveServer: vi.fn(() => null)
}));

// A directory is someone else's server: what the user typed must not reach it.
vi.mock('$lib/stores/directories', () => ({
	getDirectories: vi.fn(() => ['https://directory.test'])
}));

type FetchMock = Mock<(input: RequestInfo | URL) => Promise<Response>>;

const hit = { lat: '37.8044', lon: '-122.2712', display_name: 'Oakland, California, United States' };

describe('geocode', () => {
	let fetchMock: FetchMock;

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
});
