/**
 * Marketplace helpers: the market kinds, the item-condition vocabulary, price formatting that
 * distinguishes "no price" from "$0.00", and the filter ⇄ query-string codec.
 *
 * Currency is never defaulted here: the server resolves `[market] default_currency` when a create
 * request leaves `currency` empty, and inventing "USD" would silently override it.
 */
import { api } from '$lib/api/client';
import type { PostLike } from '$lib/api/types';

export type MarketKind = 'listing' | 'want';

/**
 * Mirrors `komun_core::models::post::ItemCondition`, pinned to the `chk_posts_item_condition` CHECK
 * constraint; anything outside this list is rejected with a 400.
 */
export type ItemCondition = 'new' | 'like_new' | 'good' | 'fair' | 'poor' | 'for_parts';

export const ITEM_CONDITION_VALUES = [
	'new',
	'like_new',
	'good',
	'fair',
	'poor',
	'for_parts'
] as const satisfies readonly ItemCondition[];

export const ITEM_CONDITION_LABELS: Record<ItemCondition, string> = {
	new: 'New',
	like_new: 'Like new',
	good: 'Good',
	fair: 'Fair',
	poor: 'Poor',
	for_parts: 'For parts'
};

export const ITEM_CONDITIONS = ITEM_CONDITION_VALUES.map((value) => ({
	value,
	label: ITEM_CONDITION_LABELS[value]
}));

export const MARKET_KINDS = ['listing', 'want'] as const satisfies readonly MarketKind[];

/** Keyed by `string` so a card can render a post whose `kind` is the wider `PostKind`. */
export const MARKET_KIND_LABELS: Record<string, string> = {
	listing: 'Listing',
	want: 'Wanted'
};

export interface MarketPost extends PostLike {
	category_label?: string | null;
	market_listed?: boolean;
	price_cents?: number | null;
	currency?: string | null;
	price_negotiable?: boolean;
	item_condition?: ItemCondition | string | null;
}

/** An absent price is not a zero price: no `price_cents` reads "Free / negotiable", a real `0` formats as zero. */
export function formatPrice(
	priceCents: number | null | undefined,
	currency?: string | null,
	negotiable = false
): string {
	if (priceCents == null || !Number.isFinite(priceCents)) return 'Free / negotiable';

	const amount = priceCents / 100;
	let formatted: string;
	if (currency) {
		try {
			formatted = new Intl.NumberFormat('en-US', {
				style: 'currency',
				currency
			}).format(amount);
		} catch {
			formatted = `${amount.toFixed(2)} ${currency}`;
		}
	} else {
		formatted = amount.toFixed(2);
	}

	return negotiable ? `${formatted} (negotiable)` : formatted;
}

export function conditionLabel(condition?: string | null): string {
	if (!condition) return '';
	return ITEM_CONDITION_LABELS[condition as ItemCondition] ?? condition;
}

/** "25.10" → 2510; empty or malformed → `null` (never `NaN` on the wire). */
export function parsePriceToCents(raw: string | number | null | undefined): number | null {
	const trimmed = String(raw ?? '').trim();
	if (!trimmed) return null;
	const value = Number(trimmed);
	if (!Number.isFinite(value) || value < 0) return null;
	return Math.round(value * 100);
}

export interface MarketFilters {
	kind?: MarketKind;
	category?: string;
	q?: string;
	min_price_cents?: number;
	max_price_cents?: number;
	currency?: string;
	item_condition?: ItemCondition;
	limit?: number;
	offset?: number;
}

const FILTER_KEYS = [
	'kind',
	'category',
	'q',
	'min_price_cents',
	'max_price_cents',
	'currency',
	'item_condition',
	'limit',
	'offset'
] as const;

/** Serialize the filters into a URL query string with **no** leading `?`. */
export function filtersToQuery(filters: MarketFilters): string {
	const params = new URLSearchParams();
	for (const key of FILTER_KEYS) {
		const value = filters[key];
		if (value === undefined || value === null || value === '') continue;
		params.set(key, String(value));
	}
	return params.toString();
}

function intParam(params: URLSearchParams, key: string): number | undefined {
	const raw = params.get(key);
	if (raw === null || raw.trim() === '') return undefined;
	const parsed = Number(raw);
	return Number.isInteger(parsed) && parsed >= 0 ? parsed : undefined;
}

/** Parse a query string (with or without a leading `?`) back into filters. */
export function queryToFilters(query: string | URLSearchParams): MarketFilters {
	const params = typeof query === 'string' ? new URLSearchParams(query) : query;
	const filters: MarketFilters = {};

	const kind = params.get('kind');
	if (kind === 'listing' || kind === 'want') filters.kind = kind;

	const category = params.get('category')?.trim();
	if (category) filters.category = category;

	const q = params.get('q')?.trim();
	if (q) filters.q = q;

	const currency = params.get('currency')?.trim();
	if (currency) filters.currency = currency;

	const condition = params.get('item_condition');
	if (condition && (ITEM_CONDITION_VALUES as readonly string[]).includes(condition)) {
		filters.item_condition = condition as ItemCondition;
	}

	const minPrice = intParam(params, 'min_price_cents');
	if (minPrice !== undefined) filters.min_price_cents = minPrice;

	const maxPrice = intParam(params, 'max_price_cents');
	if (maxPrice !== undefined) filters.max_price_cents = maxPrice;

	const limit = intParam(params, 'limit');
	if (limit !== undefined) filters.limit = limit;

	const offset = intParam(params, 'offset');
	if (offset !== undefined) filters.offset = offset;

	return filters;
}

/** The exact string-valued params `GET /api/posts` accepts. */
export function marketListParams(filters: MarketFilters): Record<string, string> {
	const params: Record<string, string> = {};
	for (const [key, value] of new URLSearchParams(filtersToQuery(filters))) {
		params[key] = value;
	}
	return params;
}

/**
 * `GET /api/posts` constrained to the market kinds. With no `kind`, two requests (one per kind)
 * are merged newest-first, so an unfiltered request never returns aid posts.
 */
export async function listMarketPosts(filters: MarketFilters = {}): Promise<MarketPost[]> {
	if (filters.kind) {
		return api.posts.list(marketListParams(filters));
	}

	const [listings, wants] = await Promise.all([
		api.posts.list(marketListParams({ ...filters, kind: 'listing' })),
		api.posts.list(marketListParams({ ...filters, kind: 'want' }))
	]);

	return [...listings, ...wants].sort(
		(a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
	);
}

export interface NewMarketPost {
	kind: MarketKind;
	category: string;
	title: string;
	body?: string | null;
	price_cents?: number | null;
	currency?: string | null;
	price_negotiable?: boolean;
	item_condition?: ItemCondition | null;
	location_name?: string | null;
	location_lat?: number | null;
	location_lon?: number | null;
	contact_method?: string | null;
	expires_at?: string | null;
}

/** `POST /api/posts` with the marketplace facet, always marked as listed. */
export async function createMarketPost(input: NewMarketPost): Promise<MarketPost> {
	const currency = input.currency?.trim();
	return api.posts.create({
		...input,
		market_listed: true,
		currency: currency ? currency.toUpperCase() : null,
		price_cents: input.price_cents ?? null,
		price_negotiable: input.price_negotiable ?? false,
		item_condition: input.item_condition ?? null
	});
}
