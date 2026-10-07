import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import MarketCard from '$lib/components/MarketCard.svelte';
import {
	filtersToQuery,
	formatPrice,
	marketListParams,
	parsePriceToCents,
	queryToFilters,
	type MarketFilters,
	type MarketPost
} from '$lib/api/market';
import { categoriesForScope, type Category } from '$lib/api/categories';

const makeMarketPost = (overrides: Partial<MarketPost> = {}): MarketPost => ({
	id: 'post-1',
	author_id: 'author-1',
	kind: 'listing',
	category: 'furniture',
	category_label: 'Furniture & Home',
	title: 'Oak dining table',
	body: 'Solid oak, seats six.',
	status: 'active',
	created_at: new Date().toISOString(),
	market_listed: true,
	price_cents: 2500,
	currency: 'USD',
	price_negotiable: false,
	item_condition: 'good',
	location_name: 'East Oakland',
	...overrides
});

describe('market price formatting', () => {
	it('formats a price from cents and currency', () => {
		expect(formatPrice(2500, 'USD')).toBe('$25.00');
	});

	it('formats a real zero as the currency zero, not as free', () => {
		expect(formatPrice(0, 'USD')).toBe('$0.00');
	});

	it('shows "Free / negotiable" when there is no price at all', () => {
		expect(formatPrice(null)).toBe('Free / negotiable');
		expect(formatPrice(undefined, 'USD')).toBe('Free / negotiable');
	});

	it('falls back to a plain amount when the currency code is unknown', () => {
		expect(formatPrice(2500, 'NOT-A-CODE')).toBe('25.00 NOT-A-CODE');
	});

	it('parses major units to cents and rejects nonsense', () => {
		expect(parsePriceToCents('25.10')).toBe(2510);
		// `bind:value` on a number input can hand back a number, not a string.
		expect(parsePriceToCents(25)).toBe(2500);
		expect(parsePriceToCents('')).toBeNull();
		expect(parsePriceToCents('abc')).toBeNull();
		expect(parsePriceToCents('-3')).toBeNull();
		expect(parsePriceToCents(null)).toBeNull();
	});
});

describe('market category derivation', () => {
	const rows: Category[] = [
		{ slug: 'electronics', label: 'Electronics & Computers', scope: 'market' },
		{ slug: 'services', label: 'Services & Labor', scope: 'both' },
		{ slug: 'shelter', label: 'Shelter', scope: 'aid' }
	];

	it('includes a `both` row in the scope=market list', () => {
		const market = categoriesForScope(rows, 'market');
		expect(market.map((c) => c.slug)).toEqual(['electronics', 'services']);
	});

	it('keeps aid-only rows out of the market list', () => {
		expect(categoriesForScope(rows, 'market').some((c) => c.slug === 'shelter')).toBe(false);
	});

	it('is the union the seed promises: market rows plus both rows', () => {
		// 15 market-only + 6 `both` = 21 for `scope=market` against the seed. The
		// count here only proves the rule; the server test pins the seed itself.
		const seed: Category[] = [
			...Array.from({ length: 15 }, (_, i) => ({
				slug: `market-${i}`,
				label: `Market ${i}`,
				scope: 'market' as const
			})),
			...Array.from({ length: 6 }, (_, i) => ({
				slug: `both-${i}`,
				label: `Both ${i}`,
				scope: 'both' as const
			})),
			...Array.from({ length: 2 }, (_, i) => ({
				slug: `aid-${i}`,
				label: `Aid ${i}`,
				scope: 'aid' as const
			}))
		];
		expect(categoriesForScope(seed, 'market')).toHaveLength(21);
	});
});

describe('market filter query string', () => {
	it('round-trips filters through the query string unchanged', () => {
		const filters: MarketFilters = {
			kind: 'listing',
			category: 'furniture',
			q: 'oak table',
			min_price_cents: 100,
			max_price_cents: 5000,
			item_condition: 'good',
			currency: 'USD',
			limit: 60,
			offset: 0
		};
		expect(queryToFilters(filtersToQuery(filters))).toEqual(filters);
	});

	it('drops empty filters instead of serialising blanks', () => {
		expect(
			filtersToQuery({ kind: 'want', category: '', q: undefined, min_price_cents: undefined })
		).toBe('kind=want');
	});

	it('ignores filter values the server would reject', () => {
		expect(queryToFilters('kind=buy&item_condition=mint&min_price_cents=-5')).toEqual({});
	});
});

describe('market radius and sort in the query string', () => {
	it('round-trips a preset radius, "any" and a distance order', () => {
		const preset: MarketFilters = { kind: 'listing', radius_km: 50, sort: 'distance' };
		expect(queryToFilters(filtersToQuery(preset))).toEqual(preset);
		const any: MarketFilters = { radius_km: 'any' };
		expect(queryToFilters(filtersToQuery(any))).toEqual(any);
	});

	it('ignores a radius outside the presets and an order outside the allow-list', () => {
		expect(queryToFilters('radius_km=7&sort=popular')).toEqual({});
		expect(queryToFilters('radius_km=-25&sort=trending')).toEqual({});
	});

	it('never puts a centre into the shareable query string', () => {
		const withCentre = {
			radius_km: 25,
			near_lat: 37.8,
			near_lon: -122.3
		} as unknown as MarketFilters;
		expect(filtersToQuery(withCentre)).toBe('radius_km=25');
	});

	it('sends no radius and no distance order without a centre', () => {
		expect(marketListParams({ kind: 'want', radius_km: 100, sort: 'distance' })).toEqual({
			kind: 'want'
		});
	});
});

describe('MarketCard', () => {
	it('renders a listing with its price, category label, condition and location', () => {
		render(MarketCard, { props: { post: makeMarketPost() } });

		expect(screen.getByText('Listing')).toBeInTheDocument();
		expect(screen.getByText('$25.00')).toBeInTheDocument();
		expect(screen.getByText('Furniture & Home')).toBeInTheDocument();
		expect(screen.getByText('Good')).toBeInTheDocument();
		expect(screen.getByText('East Oakland')).toBeInTheDocument();
		expect(screen.getByTitle('Open this post')).toHaveAttribute('href', '/p/post-1');
	});

	it('renders a wanted ad as Wanted, not as a listing', () => {
		render(MarketCard, { props: { post: makeMarketPost({ kind: 'want', title: 'Wanted: a desk' }) } });

		expect(screen.getByText('Wanted')).toBeInTheDocument();
		expect(screen.queryByText('Listing')).not.toBeInTheDocument();
	});

	it('renders a price-less post as "Free / negotiable", never "$0.00"', () => {
		render(MarketCard, { props: { post: makeMarketPost({ price_cents: null, currency: null }) } });

		expect(screen.getByText('Free / negotiable')).toBeInTheDocument();
		expect(screen.queryByText('$0.00')).not.toBeInTheDocument();
	});
});
