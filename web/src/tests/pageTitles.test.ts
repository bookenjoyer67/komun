import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { serverState } from '$lib/stores/server';
import AidPage from '../routes/aid/+page.svelte';
import ConnectPage from '../routes/connect/+page.svelte';
import SearchPage from '../routes/search/+page.svelte';

/**
 * A tab, a bookmark and a search result name the page, and the page the nav calls Aid is headed
 * Aid.
 */

const SERVER = 'https://test.komun.buzz';

vi.mock('$app/stores', async () => {
	const { writable } = await import('svelte/store');
	return {
		page: writable({ params: {}, url: new URL('http://localhost/') }),
		navigating: writable(null),
		updated: writable(false)
	};
});

vi.mock('$lib/api/client', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/client')>();
	return {
		...actual,
		api: { ...actual.api, posts: { ...actual.api.posts, list: async () => [] } }
	};
});

vi.mock('$lib/api/discovery', () => ({
	discoverAllServers: vi.fn(async () => [])
}));

async function renderLayout() {
	const { default: Layout } = await import('../routes/+layout.svelte');
	return render(Layout, {
		props: { children: createRawSnippet(() => ({ render: () => '<span>page</span>' })) }
	});
}

beforeEach(() => {
	document.title = '';
	vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 401 })));
	vi.stubGlobal(
		'matchMedia',
		vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
	);
	serverState.set({ active: SERVER, known: [] });
});

afterEach(() => {
	vi.unstubAllGlobals();
	serverState.set({ active: null, known: [] });
});

describe('each page names itself in the title', () => {
	it('titles /aid "Aid — Komun"', async () => {
		render(AidPage);

		await waitFor(() => expect(document.title).toBe('Aid — Komun'));
	});

	it('titles /connect "Connect — Komun"', async () => {
		render(ConnectPage);

		await waitFor(() => expect(document.title).toBe('Connect — Komun'));
	});

	it('titles /search "Search — Komun" and heads it with one h1', async () => {
		render(SearchPage, { props: { data: { q: '' } } });

		await waitFor(() => expect(document.title).toBe('Search — Komun'));
		expect(screen.getByRole('heading', { level: 1, name: 'Search' })).toBeInTheDocument();
	});
});

describe('/aid is headed with the label the nav gives it', () => {
	it('heads /aid with the nav link text', async () => {
		await renderLayout();
		const navLabel = screen.getByRole('link', { name: 'Aid' }).textContent?.trim();
		cleanup();

		render(AidPage);

		expect(navLabel).toBe('Aid');
		expect(screen.getByRole('heading', { level: 1 }).textContent?.trim()).toBe(navLabel);
	});
});
