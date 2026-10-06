import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { connectToServer, getActiveServer, serverState } from '$lib/stores/server';

type FetchMock = Mock<(input: RequestInfo | URL) => Promise<Response>>;

const ORIGIN = window.location.origin;
const node = { name: 'Test Node', description: 'd', version: '0.1.0', listed: false };

let fetchMock: FetchMock;

beforeEach(() => {
	fetchMock = vi.fn(async (input: RequestInfo | URL) => {
		if (String(input) === `${ORIGIN}/api/node`) {
			return new Response(JSON.stringify(node), { status: 200 });
		}
		throw new TypeError('Failed to fetch');
	});
	vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
	vi.unstubAllGlobals();
	serverState.set({ active: null, known: [] });
	localStorage.clear();
});

describe('connectToServer', () => {
	it('S1: a server on another origin is refused with the reason, and nothing is sent', async () => {
		await expect(connectToServer('https://other.example')).rejects.toThrow(
			/only talks to the server it was loaded from/
		);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('S2: an address without a scheme is refused with the reason, and nothing is sent', async () => {
		await expect(connectToServer('example.org')).rejects.toThrow(/not a full server address/i);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('S3: the own origin connects, with a trailing slash stripped', async () => {
		await expect(connectToServer(`${ORIGIN}/`)).resolves.toMatchObject({ name: 'Test Node' });
		expect(getActiveServer()).toBe(ORIGIN);
	});
});
