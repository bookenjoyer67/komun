import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { auth } from '$lib/stores/auth';
import { serverState } from '$lib/stores/server';
import type { PostLike } from '$lib/api/types';
import type { MarketPost } from '$lib/api/market';
import PostPage from '../routes/p/[id]/+page.svelte';
import AidPage from '../routes/aid/+page.svelte';
import AidCard from '$lib/components/AidCard.svelte';
import MarketCard from '$lib/components/MarketCard.svelte';

/**
 * A closed post must say so to every reader, not only to its author. A sale is stored as
 * `fulfilled` plus `sold_at`, so a sold listing and a finished need share a status.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-1';
const AUTHOR = 'user-author';
const STRANGER = 'user-stranger';
const SOLD_AT = '2026-10-07T12:00:00Z';
const SOLD_LINE = 'Sold — no longer available';
const FULFILLED_LINE = 'Fulfilled — no longer taking responses';

type Post = PostLike & { sold_at?: string | null };

const fixture = vi.hoisted(() => ({
	post: {} as Record<string, unknown>,
	list: [] as Record<string, unknown>[]
}));

vi.mock('$app/stores', async () => {
	const { writable } = await import('svelte/store');
	return {
		page: writable({ params: { id: 'post-1' }, url: new URL('http://localhost/p/post-1') }),
		navigating: writable(null),
		updated: writable(false)
	};
});

vi.mock('$lib/api/client', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/client')>();
	return {
		...actual,
		api: {
			...actual.api,
			posts: {
				...actual.api.posts,
				get: async () => ({ ...fixture.post }),
				list: async () => fixture.list.map((p) => ({ ...p })),
				update: vi.fn()
			}
		}
	};
});

const makePost = (overrides: Partial<Post> = {}): Post => ({
	id: POST_ID,
	author_id: AUTHOR,
	kind: 'need',
	category: 'food',
	title: 'Need groceries',
	body: 'Can someone help with groceries this week?',
	status: 'active',
	created_at: new Date(Date.now() - 3600000).toISOString(),
	...overrides
});

const soldListing = () =>
	makePost({ kind: 'listing', title: 'Oak dining table', status: 'fulfilled', sold_at: SOLD_AT });
const fulfilledNeed = () => makePost({ status: 'fulfilled' });

const makeMarketPost = (overrides: Partial<MarketPost> = {}): MarketPost => ({
	id: POST_ID,
	author_id: AUTHOR,
	kind: 'listing',
	category: 'furniture',
	category_label: 'Furniture & Home',
	title: 'Oak dining table',
	status: 'active',
	created_at: new Date().toISOString(),
	market_listed: true,
	price_cents: 2500,
	currency: 'USD',
	price_negotiable: false,
	...overrides
});

function signInAs(userId: string | null) {
	auth.set({
		keypair: null,
		servers: userId
			? { [SERVER]: { token: 'fake-test-token', userId, displayName: 'Test User', role: 'user' } }
			: {}
	});
}

async function renderPost(post: Post) {
	fixture.post = { ...post };
	render(PostPage);
	await screen.findByRole('heading', { name: post.title });
}

beforeEach(() => {
	vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
	serverState.set({ active: SERVER, known: [] });
	signInAs(null);
});

afterEach(() => {
	vi.unstubAllGlobals();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
	fixture.list = [];
});

describe('MarketCard: a sold listing reads as sold', () => {
	it('marks a sold listing "Sold"', () => {
		render(MarketCard, { props: { post: makeMarketPost({ status: 'fulfilled', sold_at: SOLD_AT }) } });

		expect(screen.getByText('Sold')).toBeInTheDocument();
	});

	it('marks nothing on an active listing', () => {
		render(MarketCard, { props: { post: makeMarketPost() } });

		expect(screen.queryByText('Sold')).not.toBeInTheDocument();
	});

	it('marks a fulfilled want "Fulfilled", because a want is not sold', () => {
		render(MarketCard, { props: { post: makeMarketPost({ kind: 'want', status: 'fulfilled' }) } });

		expect(screen.getByText('Fulfilled')).toBeInTheDocument();
		expect(screen.queryByText('Sold')).not.toBeInTheDocument();
	});
});

describe('post page: a closed post says so and takes no response', () => {
	it('tells a stranger a sold listing is no longer available, with no offer button', async () => {
		signInAs(STRANGER);
		await renderPost(soldListing());

		expect(screen.getByText(SOLD_LINE)).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'Make an offer' })).not.toBeInTheDocument();
	});

	it('tells a stranger a fulfilled need takes no responses, with no "I can help"', async () => {
		signInAs(STRANGER);
		await renderPost(fulfilledNeed());

		expect(screen.getByText(FULFILLED_LINE)).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
	});

	it('tells an anonymous reader a fulfilled need takes no responses', async () => {
		await renderPost(fulfilledNeed());

		expect(screen.getByText(FULFILLED_LINE)).toBeInTheDocument();
	});

	it.each([
		['paused', () => makePost({ status: 'matched' })],
		['sold', soldListing],
		['fulfilled', fulfilledNeed]
	])('a %s post carries exactly one status line', async (_name, make) => {
		signInAs(STRANGER);
		await renderPost(make());

		expect(screen.getAllByRole('status')).toHaveLength(1);
	});
});

describe('AidCard: a fulfilled post shows its status to every reader', () => {
	it('shows "Fulfilled" to a stranger, with no "I can help"', () => {
		signInAs(STRANGER);
		render(AidCard, { props: { post: fulfilledNeed() } });

		expect(screen.getByText('Fulfilled')).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
	});

	it('shows "Fulfilled" to an anonymous reader, with no "I can help"', () => {
		render(AidCard, { props: { post: fulfilledNeed() } });

		expect(screen.getByText('Fulfilled')).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
	});

	it('shows the author "Fulfilled" once and no Fulfill, Edit or Delete', () => {
		signInAs(AUTHOR);
		render(AidCard, {
			props: { post: fulfilledNeed(), onFulfill: vi.fn(), onEdit: vi.fn(), onDelete: vi.fn() }
		});

		expect(screen.getAllByText('Fulfilled')).toHaveLength(1);
		for (const name of ['Fulfill', 'Edit', 'Delete']) {
			expect(screen.queryByRole('button', { name })).not.toBeInTheDocument();
		}
	});

	it('marks a sold listing "Sold", not "Fulfilled"', () => {
		signInAs(STRANGER);
		render(AidCard, { props: { post: soldListing() } });

		expect(screen.getByText('Sold')).toBeInTheDocument();
		expect(screen.queryByText('Fulfilled')).not.toBeInTheDocument();
	});
});

describe('/aid: a fulfilled post shows its status to every reader', () => {
	it('shows "Fulfilled" to a stranger', async () => {
		signInAs(STRANGER);
		fixture.list = [{ ...fulfilledNeed(), title: 'Done need' }];
		render(AidPage);
		await screen.findByText('Done need');

		expect(screen.getByText('Fulfilled')).toBeInTheDocument();
	});

	it('shows the author "Fulfilled" once and no Fulfill control', async () => {
		signInAs(AUTHOR);
		fixture.list = [{ ...fulfilledNeed(), title: 'Done need' }];
		render(AidPage);
		await screen.findByText('Done need');

		expect(screen.getAllByText('Fulfilled')).toHaveLength(1);
		expect(screen.queryByRole('button', { name: 'Fulfill' })).not.toBeInTheDocument();
	});

	it('marks a sold listing "Sold", not "Fulfilled"', async () => {
		signInAs(STRANGER);
		fixture.list = [{ ...soldListing() }];
		render(AidPage);
		await screen.findByText('Oak dining table');

		expect(screen.getByText('Sold')).toBeInTheDocument();
		expect(screen.queryByText('Fulfilled')).not.toBeInTheDocument();
	});
});
