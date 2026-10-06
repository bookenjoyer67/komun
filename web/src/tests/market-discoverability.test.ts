import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { formatPrice, isMarketKind } from '$lib/api/market';

/**
 * F1: the marketplace is reachable and sale intent has a door. Each assertion here is about what a
 * person can see or do in the app — a nav entry, a back-link, an affordance on the composer the
 * landing page actually reaches.
 *
 * F8: a free listing must not read as a bare `0.00` on the card a buyer scans.
 */

const ORIGIN = 'http://localhost';

const postKind = vi.hoisted(() => ({ value: 'listing' as string }));

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
				get: async () => ({
					id: 'post-1',
					author_id: 'author-1',
					kind: postKind.value,
					category: 'furniture',
					title: 'Oak dining table',
					body: 'Solid oak, seats six.',
					status: 'active',
					created_at: new Date().toISOString(),
					market_listed: isMarketKind(postKind.value),
					price_cents: 2500,
					currency: 'USD',
					price_negotiable: false
				})
			}
		}
	};
});

function stubBrowser() {
	vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 401 })));
	vi.stubGlobal(
		'matchMedia',
		vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
	);
}

async function renderLayout() {
	const { default: Layout } = await import('../routes/+layout.svelte');
	return render(Layout, {
		props: { children: createRawSnippet(() => ({ render: () => '<span>page</span>' })) }
	});
}

beforeEach(async () => {
	stubBrowser();
	const { serverState } = await import('$lib/stores/server');
	serverState.set({ active: ORIGIN, known: [] });
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('F1: the nav names the market and stops mislabelling the aid feed', () => {
	it('offers a Market entry that points at /market', async () => {
		await renderLayout();

		const market = screen.getByRole('link', { name: 'Market' });
		expect(market.getAttribute('href')).toBe('/market');
	});

	it('no longer calls the aid feed "Listings"', async () => {
		await renderLayout();

		expect(screen.queryByRole('link', { name: 'Listings' })).toBeNull();
		expect(screen.getByRole('link', { name: 'Aid' }).getAttribute('href')).toBe('/aid');
	});
});

describe('F1: a post returns to the feed it belongs to', () => {
	async function renderPost(kind: string) {
		postKind.value = kind;
		const { default: PostPage } = await import('../routes/p/[id]/+page.svelte');
		return render(PostPage);
	}

	it('sends a market post back to /market', async () => {
		await renderPost('listing');

		const back = await screen.findByRole('link', { name: /all listings/i });
		expect(back.getAttribute('href')).toBe('/market');
	});

	it('sends a wanted post back to /market too', async () => {
		await renderPost('want');

		const back = await screen.findByRole('link', { name: /all listings/i });
		expect(back.getAttribute('href')).toBe('/market');
	});

	it('keeps an aid post on /aid', async () => {
		await renderPost('need');

		const back = await screen.findByRole('link', { name: /all listings/i });
		expect(back.getAttribute('href')).toBe('/aid');
	});
});

describe('F1: the composer the landing page reaches offers a way to sell', () => {
	it('routes sale intent from /aid/new to /market/new', async () => {
		const { default: AidNew } = await import('../routes/aid/new/+page.svelte');
		render(AidNew);

		const toMarket = screen.getByRole('link', { name: /market/i });
		expect(toMarket.getAttribute('href')).toBe('/market/new');
	});
});

describe('isMarketKind', () => {
	it('is true for the market facets and false for the aid ones', () => {
		expect(isMarketKind('listing')).toBe(true);
		expect(isMarketKind('want')).toBe(true);
		expect(isMarketKind('need')).toBe(false);
		expect(isMarketKind('offer')).toBe(false);
		expect(isMarketKind('resource')).toBe(false);
		expect(isMarketKind(null)).toBe(false);
		expect(isMarketKind(undefined)).toBe(false);
	});
});

describe('F8: a zero price is never a bare number', () => {
	it('reads Free when a zero price carries no currency', () => {
		expect(formatPrice(0, null)).toBe('Free');
		expect(formatPrice(0, undefined, true)).toBe('Free / negotiable');
	});

	it('keeps the currency zero when a currency is set', () => {
		expect(formatPrice(0, 'USD')).toBe('$0.00');
		expect(formatPrice(0, 'USD', true)).toBe('$0.00 (negotiable)');
	});

	it('names the missing currency instead of rendering a bare number', () => {
		expect(formatPrice(2500, null)).toBe('25.00 (no currency)');
		expect(formatPrice(2500, null, true)).toBe('25.00 (no currency) (negotiable)');
	});

	it('still reads "Free / negotiable" when no price was given at all', () => {
		expect(formatPrice(null)).toBe('Free / negotiable');
		expect(formatPrice(undefined, 'USD')).toBe('Free / negotiable');
	});
});
