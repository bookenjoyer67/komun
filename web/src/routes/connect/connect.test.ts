import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { get } from 'svelte/store';
import { serverState } from '$lib/stores/server';
import { discoverAllServers } from '$lib/api/discovery';
import ConnectPage from './+page.svelte';

vi.mock('$lib/api/discovery', () => ({
	discoverAllServers: vi.fn(async () => [])
}));

const ORIGIN = window.location.origin;
const node = { name: 'Test Node', description: 'd', version: '0.1.0', listed: false };

async function connectTo(address: string) {
	const user = userEvent.setup();
	render(ConnectPage);
	await user.type(screen.getByRole('textbox'), address);
	await user.click(screen.getByRole('button', { name: 'Connect' }));
}

async function expectRefusalNaming(target: string) {
	const alert = await screen.findByRole('alert');
	expect(alert).toHaveTextContent(/not connected/i);
	expect(alert).toHaveTextContent(ORIGIN);
	expect(alert).toHaveTextContent(target);
	expect(alert.textContent?.trim()).not.toBe('Failed to fetch');
}

beforeEach(() => {
	vi.stubGlobal(
		'fetch',
		vi.fn(async (input: RequestInfo | URL) => {
			if (String(input) === `${ORIGIN}/api/node`) {
				return new Response(JSON.stringify(node), { status: 200 });
			}
			throw new TypeError('Failed to fetch');
		})
	);
});

afterEach(() => {
	vi.unstubAllGlobals();
	serverState.set({ active: null, known: [] });
	localStorage.clear();
});

describe('connect page', () => {
	it('T1: an address without a scheme is answered in the page', async () => {
		await connectTo('example.org');

		const alert = await screen.findByRole('alert');
		expect(alert).toHaveTextContent(/not connected/i);
		expect(alert).toHaveTextContent(/https:\/\//);
	});

	it('T2: a server on another origin is refused with the reason', async () => {
		await connectTo('https://other.example');

		await expectRefusalNaming('https://other.example');
	});

	it('T2b: a plain-http server on another origin is refused with the reason', async () => {
		await connectTo('http://other.example:8080');

		await expectRefusalNaming('http://other.example:8080');
	});

	it('T3: the own server still connects', async () => {
		await connectTo(ORIGIN);

		// The name also appears in the known-servers list once the connection is recorded.
		expect(await screen.findByRole('heading', { name: 'Test Node' })).toBeInTheDocument();
		expect(screen.queryByRole('alert')).not.toBeInTheDocument();
		expect(get(serverState).active).toBe(ORIGIN);
	});

	it('T4: a listed server on another origin is refused visibly', async () => {
		vi.mocked(discoverAllServers).mockResolvedValueOnce([{ url: 'https://listed.example', name: 'Listed' }]);
		const user = userEvent.setup();
		render(ConnectPage);

		await user.click(await screen.findByRole('button', { name: /Listed/ }));

		await expectRefusalNaming('https://listed.example');
	});
});
