/**
 * Marketplace helpers: the market kinds, the item-condition vocabulary, price formatting that
 * distinguishes "no price" from "$0.00", and the filter ⇄ query-string codec.
 *
 * Currency is never defaulted here: the server resolves `[market] default_currency` when a create
 * request leaves `currency` empty, and inventing "USD" would silently override it.
 */
import { api } from '$lib/api/client';
import type { PostLike } from '$lib/api/types';
import { coarsenCoordinate } from '$lib/geo';

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

/**
 * Is this post kind one of the market facets? Keyed by `string` so a caller holding the wider
 * `PostKind` can branch on it without narrowing first.
 */
export function isMarketKind(kind?: string | null): boolean {
	return kind != null && (MARKET_KINDS as readonly string[]).includes(kind);
}

export interface MarketPost extends PostLike {
	category_label?: string | null;
	market_listed?: boolean;
	price_cents?: number | null;
	currency?: string | null;
	price_negotiable?: boolean;
	item_condition?: ItemCondition | string | null;
	sold_at?: string | null;
	/** Present only when the request carried a centre and the post has a location. */
	distance_km?: number | null;
}

/**
 * Both ends of a distance sit on the 0.1° grid, so a distance is good to about ±13 km. The presets
 * start at town scale for that reason; a smaller one would promise a neighbourhood the data cannot
 * resolve.
 */
export const RADIUS_PRESETS_KM = [15, 25, 50, 100] as const;
export type RadiusPresetKm = (typeof RADIUS_PRESETS_KM)[number];
export type RadiusChoice = RadiusPresetKm | 'any';
export const DEFAULT_RADIUS_KM: RadiusPresetKm = 25;

export const RADIUS_CHOICES: { value: RadiusChoice; label: string }[] = [
	...RADIUS_PRESETS_KM.map((km) => ({ value: km, label: `Within ${km} km` })),
	{ value: 'any', label: 'Any distance' }
];

function isRadiusPreset(value: number): value is RadiusPresetKm {
	return (RADIUS_PRESETS_KM as readonly number[]).includes(value);
}

/** Reads a `<select>` value back into a choice; anything unrecognised is the default. */
export function parseRadiusChoice(raw: string): RadiusChoice {
	if (raw === 'any') return 'any';
	const km = Number(raw);
	return isRadiusPreset(km) ? km : DEFAULT_RADIUS_KM;
}

/** The next wider choice, ending at "any"; `null` once nothing is wider. */
export function nextRadius(current: RadiusChoice): RadiusChoice | null {
	if (current === 'any') return null;
	const index = RADIUS_PRESETS_KM.indexOf(current);
	return RADIUS_PRESETS_KM[index + 1] ?? 'any';
}

/** Always approximate: a precise figure would claim an accuracy the grid does not have. */
export function distanceLabel(km: number | null | undefined): string {
	if (km == null || !Number.isFinite(km)) return '';
	return `~${Math.round(km)} km`;
}

export interface Centre {
	lat: number | null;
	lon: number | null;
}

/**
 * The `near_lat`/`near_lon`/`radius_km` params for a centre. Without a whole centre there are no
 * distance params at all, because the server refuses half a centre and a radius with none. The
 * centre is coarsened before it leaves the browser: the server never needs the exact point.
 */
export function radiusParams(
	centre: Centre | null | undefined,
	radius: RadiusChoice = DEFAULT_RADIUS_KM
): Record<string, string> {
	const lat = centre?.lat;
	const lon = centre?.lon;
	if (lat == null || lon == null || !Number.isFinite(lat) || !Number.isFinite(lon)) return {};
	const params: Record<string, string> = {
		near_lat: String(coarsenCoordinate(lat)),
		near_lon: String(coarsenCoordinate(lon))
	};
	if (radius !== 'any') params.radius_km = String(radius);
	return params;
}

/**
 * An absent price is not a zero price: no `price_cents` reads "Free / negotiable", a real `0`
 * formats as its currency's zero, and a zero with no currency reads "Free".
 *
 * Currency is never invented — the server resolves `[market] default_currency` and keeps none when
 * there is none. So an amount with no currency names the gap instead of rendering as a bare number
 * a reader cannot place.
 */
export function formatPrice(
	priceCents: number | null | undefined,
	currency?: string | null,
	negotiable = false
): string {
	if (priceCents == null || !Number.isFinite(priceCents)) return 'Free / negotiable';

	const amount = priceCents / 100;
	if (priceCents === 0 && !currency) return negotiable ? 'Free / negotiable' : 'Free';

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
		formatted = `${amount.toFixed(2)} (no currency)`;
	}

	return negotiable ? `${formatted} (negotiable)` : formatted;
}

export function conditionLabel(condition?: string | null): string {
	if (!condition) return '';
	return ITEM_CONDITION_LABELS[condition as ItemCondition] ?? condition;
}

/**
 * A sale is stored as `fulfilled`, the status a finished want or need also carries, so the kind
 * decides the word: only a listing is sold.
 */
export function closedLabel(post: {
	kind?: string;
	status?: string;
	sold_at?: string | null;
}): string {
	const sold = post.kind === 'listing' && (post.sold_at != null || post.status === 'fulfilled');
	if (sold) return 'Sold';
	if (post.status === 'fulfilled') return 'Fulfilled';
	return '';
}

/** "25.10" → 2510; empty or malformed → `null` (never `NaN` on the wire). */
export function parsePriceToCents(raw: string | number | null | undefined): number | null {
	const trimmed = String(raw ?? '').trim();
	if (!trimmed) return null;
	const value = Number(trimmed);
	if (!Number.isFinite(value) || value < 0) return null;
	return Math.round(value * 100);
}

export type FeedSort = 'recency' | 'distance';

/**
 * The URL-facing filters. The centre is deliberately not one of them: it comes from the saved
 * location at request time, so a shared link never carries where its sender lives.
 */
export interface MarketFilters {
	kind?: MarketKind;
	category?: string;
	q?: string;
	min_price_cents?: number;
	max_price_cents?: number;
	currency?: string;
	item_condition?: ItemCondition;
	radius_km?: RadiusChoice;
	sort?: FeedSort;
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
	'radius_km',
	'sort',
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

	const radius = params.get('radius_km');
	if (radius === 'any') {
		filters.radius_km = 'any';
	} else {
		const km = intParam(params, 'radius_km');
		if (km !== undefined && isRadiusPreset(km)) filters.radius_km = km;
	}

	const sort = params.get('sort');
	if (sort === 'recency' || sort === 'distance') filters.sort = sort;

	const limit = intParam(params, 'limit');
	if (limit !== undefined) filters.limit = limit;

	const offset = intParam(params, 'offset');
	if (offset !== undefined) filters.offset = offset;

	return filters;
}

/**
 * The exact string-valued params `GET /api/posts` accepts. A radius or a distance order without a
 * centre is dropped rather than sent, because the server refuses both.
 */
export function marketListParams(
	filters: MarketFilters,
	centre?: Centre | null
): Record<string, string> {
	const { radius_km, sort, ...rest } = filters;
	const params: Record<string, string> = {};
	for (const [key, value] of new URLSearchParams(filtersToQuery(rest))) {
		params[key] = value;
	}
	const distance = radiusParams(centre, radius_km ?? DEFAULT_RADIUS_KM);
	Object.assign(params, distance);
	if (sort === 'recency' || (sort === 'distance' && distance.near_lat)) params.sort = sort;
	return params;
}

function byDistanceThenNewest(a: MarketPost, b: MarketPost): number {
	const da = a.distance_km ?? Number.POSITIVE_INFINITY;
	const db = b.distance_km ?? Number.POSITIVE_INFINITY;
	if (da !== db) return da - db;
	return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
}

/**
 * `GET /api/posts` constrained to the market kinds. With no `kind`, two requests (one per kind)
 * are merged, so an unfiltered request never returns aid posts. The merge keeps the requested
 * order: nearest first under a distance order, with unlocated posts last, and newest first
 * otherwise.
 */
export async function listMarketPosts(
	filters: MarketFilters = {},
	centre?: Centre | null
): Promise<MarketPost[]> {
	if (filters.kind) {
		return api.posts.list(marketListParams(filters, centre));
	}

	const [listings, wants] = await Promise.all([
		api.posts.list(marketListParams({ ...filters, kind: 'listing' }, centre)),
		api.posts.list(marketListParams({ ...filters, kind: 'want' }, centre))
	]);

	const merged: MarketPost[] = [...listings, ...wants];
	if (filters.sort === 'distance') return merged.sort(byDistanceThenNewest);
	return merged.sort(
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
