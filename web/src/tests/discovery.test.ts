import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { discoverNearbyServers } from '$lib/api/discovery';

vi.mock('$lib/stores/directories', () => ({
	getDirectories: vi.fn(() => ['https://directory.test'])
}));

vi.mock('$lib/stores/location', () => ({
	getLocation: vi.fn(() => ({ name: 'Oakland', lat: 37.80443, lon: -122.27121 }))
}));

type FetchMock = Mock<(input: RequestInfo | URL) => Promise<Response>>;

describe('discoverNearbyServers', () => {
	let fetchMock: FetchMock;

	beforeEach(() => {
		fetchMock = vi.fn(async (_input: RequestInfo | URL) => new Response('[]', { status: 200 }));
		vi.stubGlobal('fetch', fetchMock);
	});

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('sends the directory a location rounded to the grid, never the exact one', async () => {
		await discoverNearbyServers();

		expect(fetchMock).toHaveBeenCalledTimes(1);
		const sent = String(fetchMock.mock.calls[0][0]);
		const url = new URL(sent);
		expect(url.origin).toBe('https://directory.test');
		expect(url.pathname).toBe('/api/directory');
		expect(url.searchParams.get('lat')).toBe('37.8');
		expect(url.searchParams.get('lon')).toBe('-122.3');
		expect(sent).not.toContain('37.80443');
		expect(sent).not.toContain('122.27121');
	});
});
