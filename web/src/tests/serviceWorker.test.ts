// @vitest-environment node
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';

vi.mock('$service-worker', () => ({
	build: ['/_app/immutable/start.js'],
	files: ['/favicon.svg'],
	version: 'test'
}));

const ORIGIN = 'https://komun.test';

interface FetchEventStub {
	request: { url: string; method: string; mode: string };
	respondWith: ReturnType<typeof vi.fn>;
}

type Listener = (event: FetchEventStub) => void;

const listeners = new Map<string, Listener>();
const fetchMock = vi.fn(async () => new Response(''));

function dispatch(url: string, mode: string): FetchEventStub {
	const handler = listeners.get('fetch');
	if (!handler) throw new Error('the worker registered no fetch listener');
	const event: FetchEventStub = { request: { url, method: 'GET', mode }, respondWith: vi.fn() };
	handler(event);
	return event;
}

beforeAll(async () => {
	vi.stubGlobal('self', {
		location: new URL(`${ORIGIN}/`),
		addEventListener: (type: string, listener: Listener) => listeners.set(type, listener),
		skipWaiting: vi.fn(async () => undefined),
		clients: { claim: vi.fn(async () => undefined) }
	});
	vi.stubGlobal('caches', {
		match: vi.fn(async () => undefined),
		open: vi.fn(async () => ({
			put: vi.fn(async () => undefined),
			addAll: vi.fn(async () => undefined)
		})),
		keys: vi.fn(async () => []),
		delete: vi.fn(async () => true)
	});
	vi.stubGlobal('fetch', fetchMock);
	// A literal specifier would pull the worker's no-default-lib/webworker references into svelte-check's program.
	const workerModule = '../service-worker';
	await import(/* @vite-ignore */ workerModule);
});

beforeEach(() => {
	fetchMock.mockClear();
});

describe('service worker fetch handler', () => {
	it.each([
		['a map tile', 'https://tile.openstreetmap.org/11/510/785.png', 'no-cors'],
		['a precached path on another origin', 'https://tile.openstreetmap.org/favicon.svg', 'no-cors'],
		['a cacheable API path on another origin', 'https://other.example/api/posts', 'cors']
	])('leaves %s to the browser', (_label, url, mode) => {
		const event = dispatch(url, mode);

		expect(event.respondWith).not.toHaveBeenCalled();
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('still answers a precached same-origin file', () => {
		const event = dispatch(`${ORIGIN}/favicon.svg`, 'no-cors');

		expect(event.respondWith).toHaveBeenCalledTimes(1);
	});

	it('still answers a same-origin file that is not precached', () => {
		const event = dispatch(`${ORIGIN}/x.png`, 'no-cors');

		expect(event.respondWith).toHaveBeenCalledTimes(1);
	});
});
