import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { tick } from 'svelte';

/**
 * A search can be saved from the search page and managed from the account page. What leaves the
 * browser is the submitted query and the coarse cell of the centre, never the exact point, and
 * saving the same search twice is answered by the server's refusal rather than a second row.
 */

vi.mock('$lib/api/client', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/client')>();
	return {
		...actual,
		api: { ...actual.api, posts: { ...actual.api.posts, list: vi.fn(async () => []) } }
	};
});

import { auth } from '$lib/stores/auth';
import { serverState } from '$lib/stores/server';
import { clearLocation, location as savedLocation } from '$lib/stores/location';
import SearchPage from '../routes/search/+page.svelte';
import AccountPage from '../routes/account/+page.svelte';

const SERVER = 'https://test.komun.buzz';
const SAVED_PATH = '/api/me/saved-searches';

// An exact point; what leaves the browser must be its 0.1-degree cell.
const HOME = { lat: 37.80443, lon: -122.27121 };

interface Call {
	url: string;
	method: string;
	body: unknown;
}

let calls: Call[] = [];
let saved: Record<string, unknown>[] = [];
let answer: ((url: string, method: string) => Response | undefined) | null = null;

function json(data: unknown, status = 200): Response {
	return new Response(JSON.stringify(data), {
		status,
		headers: { 'Content-Type': 'application/json' }
	});
}

function route(url: string, method: string): Response {
	const special = answer?.(url, method);
	if (special) return special;
	const path = new URL(url).pathname;
	if (path === '/api/search/users') return json([]);
	if (path === '/api/me/moderation') return json([]);
	if (path === '/api/auth/me') return json({ display_name: 'Test User', email_verified: true });
	if (path === SAVED_PATH && method === 'GET') return json(saved);
	if (path === SAVED_PATH && method === 'POST') return json(makeSaved({ id: 'ss-new', label: null, q: 'ladder' }), 201);
	if (path.startsWith(`${SAVED_PATH}/`) && method === 'DELETE') return json({ status: 'deleted' });
	return json({ error: 'not found' }, 404);
}

function sent(method: string, path: string): Call[] {
	return calls.filter((c) => c.method === method && new URL(c.url).pathname === path);
}

const makeSaved = (overrides: Record<string, unknown> = {}) => ({
	id: 'ss-1',
	q: 'wool',
	kind: null,
	category: null,
	near_lat: 37.8,
	near_lon: -122.3,
	radius_km: 25,
	created_at: '2026-10-01T09:00:00Z',
	...overrides
});

function signIn() {
	auth.set({
		keypair: null,
		servers: { [SERVER]: { token: 'fake-test-token', userId: 'user-1', displayName: 'Test User', role: 'user' } }
	});
}

// A negative assertion is only meaningful once queued effects and the fetches they start have run.
async function settle() {
	await tick();
	await new Promise((resolve) => setTimeout(resolve, 0));
	await tick();
}

beforeEach(() => {
	calls = [];
	saved = [];
	answer = null;
	vi.stubGlobal(
		'fetch',
		vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			const url = String(input);
			const method = init?.method ?? 'GET';
			const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
			calls.push({ url, method, body });
			return route(url, method);
		})
	);
	vi.stubGlobal(
		'matchMedia',
		vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }))
	);
	serverState.set({ active: SERVER, known: [] });
	signIn();
});

afterEach(() => {
	vi.unstubAllGlobals();
	clearLocation();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
});

describe('search page: save this search', () => {
	beforeEach(() => {
		savedLocation.set({ name: 'Home', lat: HOME.lat, lon: HOME.lon });
	});

	it('offers the save control once a query has been submitted, and not before', async () => {
		const { rerender } = render(SearchPage, { props: { data: { q: '' } } });
		await settle();
		expect(screen.queryByRole('button', { name: 'Save this search' })).not.toBeInTheDocument();

		await rerender({ data: { q: 'ladder' } });

		expect(await screen.findByRole('button', { name: 'Save this search' })).toBeEnabled();
	});

	it('saves the submitted query with the coarse centre and the chosen radius', async () => {
		const user = userEvent.setup();
		render(SearchPage, { props: { data: { q: 'ladder' } } });

		await user.click(await screen.findByRole('button', { name: 'Save this search' }));

		expect(await screen.findByText(/Search saved/)).toBeInTheDocument();
		const posted = sent('POST', SAVED_PATH);
		expect(posted).toHaveLength(1);
		expect(posted[0].body).toEqual({ q: 'ladder', near_lat: 37.8, near_lon: -122.3, radius_km: 25 });
	});

	it('saves a centre with no radius when the distance is "any"', async () => {
		const user = userEvent.setup();
		render(SearchPage, { props: { data: { q: 'ladder' } } });

		await user.selectOptions(await screen.findByRole('combobox'), 'any');
		await user.click(await screen.findByRole('button', { name: 'Save this search' }));

		await waitFor(() => expect(sent('POST', SAVED_PATH)).toHaveLength(1));
		expect(sent('POST', SAVED_PATH)[0].body).toEqual({ q: 'ladder', near_lat: 37.8, near_lon: -122.3 });
	});

	it("shows the server's refusal when the search is already saved, and stays usable", async () => {
		const user = userEvent.setup();
		answer = (url, method) =>
			method === 'POST' && new URL(url).pathname === SAVED_PATH
				? json({ error: 'you have already saved this search' }, 409)
				: undefined;
		render(SearchPage, { props: { data: { q: 'ladder' } } });

		await user.click(await screen.findByRole('button', { name: 'Save this search' }));

		expect(await screen.findByRole('alert')).toHaveTextContent('already saved');
		expect(screen.queryByText(/Search saved/)).not.toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'Save this search' })).toBeEnabled();
	});
});

describe('account page: saved searches', () => {
	async function savedRegion(): Promise<HTMLElement> {
		render(AccountPage);
		return screen.findByRole('region', { name: 'Saved searches' });
	}

	it('lists each saved search with its words and its area', async () => {
		saved = [makeSaved(), makeSaved({ id: 'ss-2', q: 'ladder', radius_km: 50 })];

		const region = await savedRegion();

		const items = await within(region).findAllByRole('listitem');
		expect(items).toHaveLength(2);
		expect(items[0]).toHaveTextContent('wool');
		expect(items[0]).toHaveTextContent('25 km');
		expect(items[1]).toHaveTextContent('ladder');
		expect(items[1]).toHaveTextContent('50 km');
	});

	it('deletes a saved search and takes it off the list', async () => {
		const user = userEvent.setup();
		saved = [makeSaved(), makeSaved({ id: 'ss-2', q: 'ladder' })];
		const region = await savedRegion();
		const [wool] = await within(region).findAllByRole('listitem');

		await user.click(within(wool).getByRole('button', { name: /delete/i }));

		await waitFor(() => expect(within(region).getAllByRole('listitem')).toHaveLength(1));
		expect(within(region).queryByText('wool')).not.toBeInTheDocument();
		expect(sent('DELETE', `${SAVED_PATH}/ss-1`)).toHaveLength(1);
	});

	it('keeps the row and says so when a delete fails', async () => {
		const user = userEvent.setup();
		saved = [makeSaved()];
		answer = (url, method) =>
			method === 'DELETE' && new URL(url).pathname === `${SAVED_PATH}/ss-1`
				? json({ error: 'boom' }, 500)
				: undefined;
		const region = await savedRegion();
		const [wool] = await within(region).findAllByRole('listitem');

		await user.click(within(wool).getByRole('button', { name: /delete/i }));

		expect(await within(region).findByRole('alert')).toBeInTheDocument();
		expect(within(region).getAllByRole('listitem')).toHaveLength(1);
	});

	it('says plainly when nothing has been saved', async () => {
		const region = await savedRegion();

		expect(await within(region).findByText(/no saved searches/i)).toBeInTheDocument();
	});

	it('says so when the saved searches cannot be read', async () => {
		answer = (url, method) =>
			method === 'GET' && new URL(url).pathname === SAVED_PATH ? json({ error: 'boom' }, 500) : undefined;

		const region = await savedRegion();

		expect(await within(region).findByRole('alert')).toHaveTextContent(/could not load your saved searches/i);
	});
});
