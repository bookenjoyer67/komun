import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('$lib/stores/server', () => ({
	getActiveServer: vi.fn(() => 'https://node.test'),
	setActiveServer: vi.fn(),
	servers: { subscribe: vi.fn() }
}));

vi.mock('$lib/stores/auth', () => ({
	getToken: vi.fn(() => null)
}));

import { api } from '$lib/api/client';

const CONSOLE_METHODS = ['log', 'info', 'warn', 'error', 'debug'] as const;

function reply(status: number, body: unknown) {
	return {
		ok: status >= 200 && status < 300,
		status,
		statusText: `status ${status}`,
		json: async () => body
	};
}

// Every request shape the client has: a plain read, a read that may carry a session, a failing
// JSON request and a failing multipart upload.
const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
	const method = init?.method ?? 'GET';
	if (method === 'DELETE') return reply(404, { error: 'nope' });
	if (url.endsWith('/images')) return reply(500, { error: 'nope' });
	return reply(200, url.endsWith('/posts') ? [] : { id: 'x' });
});

async function exercise() {
	const list = await api.posts.list();
	const one = await api.posts.get('x');
	const removed = await api.posts.delete('x').then(
		() => 'resolved',
		(e: Error) => e.message
	);
	const file = new File(['img'], 'a.png', { type: 'image/png' });
	const uploaded = await api.posts.addImages('x', [file]).then(
		() => 'resolved',
		(e: Error) => e.message
	);
	return { list, one, removed, uploaded };
}

describe('api client', () => {
	let spies: Array<{ mockRestore(): void }> = [];

	beforeEach(() => {
		fetchMock.mockClear();
		vi.stubGlobal('fetch', fetchMock);
		spies = CONSOLE_METHODS.map((m) => vi.spyOn(console, m).mockImplementation(() => {}));
	});

	afterEach(() => {
		for (const spy of spies) spy.mockRestore();
		vi.unstubAllGlobals();
	});

	it('runs every request path against the active server', async () => {
		const result = await exercise();

		expect(fetchMock).toHaveBeenCalledWith('https://node.test/api/posts', expect.anything());
		expect(fetchMock).toHaveBeenCalledWith('https://node.test/api/posts/x', expect.anything());
		expect(fetchMock).toHaveBeenCalledWith(
			'https://node.test/api/posts/x',
			expect.objectContaining({ method: 'DELETE' })
		);
		expect(fetchMock).toHaveBeenCalledWith(
			'https://node.test/api/posts/x/images',
			expect.objectContaining({ method: 'POST' })
		);
		expect(result.list).toEqual([]);
		expect(result.one).toEqual({ id: 'x' });
		expect(result.removed).toBe('nope');
		expect(result.uploaded).toBe('nope');
	});

	// Absence means something only because the test above proves the same calls ran.
	it('writes nothing to the console on any path', async () => {
		await exercise();

		CONSOLE_METHODS.forEach((m, i) => {
			expect(spies[i], `console.${m}`).not.toHaveBeenCalled();
		});
	});
});
