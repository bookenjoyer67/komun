/**
 * The category taxonomy — fetched from the server, never compiled into the client. There is
 * deliberately no hardcoded category list in the market code: the table is the source of truth.
 */
import { getActiveServer } from '$lib/stores/server';

export type CategoryScope = 'aid' | 'market' | 'both';

export interface Category {
	slug: string;
	label: string;
	scope: CategoryScope;
}

/**
 * `scope` is a union, not an equality: asking for `market` also keeps the `both` rows, matching
 * `db::categories::list`.
 */
export function categoriesForScope(rows: Category[], scope: CategoryScope): Category[] {
	return rows.filter((row) => row.scope === scope || row.scope === 'both');
}

/**
 * The human label for a post's category, falling back to the raw slug when the taxonomy has no
 * row for it. `category_label` comes from the server's `categories` join, so no client keeps a
 * second copy of the mapping.
 */
export function categoryLabel(post: { category: string; category_label?: string | null }): string {
	return post.category_label || post.category;
}

function isCategory(value: unknown): value is Category {
	if (typeof value !== 'object' || value === null) return false;
	const candidate = value as Record<string, unknown>;
	return (
		typeof candidate.slug === 'string' &&
		typeof candidate.label === 'string' &&
		(candidate.scope === 'aid' || candidate.scope === 'market' || candidate.scope === 'both')
	);
}

/** `GET /api/categories?scope=` — public, active rows only. */
export async function fetchCategories(scope: CategoryScope): Promise<Category[]> {
	const base = getActiveServer();
	if (!base) throw new Error('Not connected to a server');

	const res = await fetch(`${base}/api/categories?scope=${scope}`);
	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		throw new Error(err.error || `Failed to load categories (${res.status})`);
	}

	const rows: unknown = await res.json();
	if (!Array.isArray(rows)) throw new Error('categories response was not a list');
	return rows.filter(isCategory);
}

/**
 * Fetched once per page load; `/market` and `/market/new` share the cache. A failed fetch clears
 * it so the next caller retries rather than re-throwing a stale rejection.
 */
let marketCategories: Promise<Category[]> | null = null;

export function fetchMarketCategories(): Promise<Category[]> {
	if (!marketCategories) {
		marketCategories = fetchCategories('market')
			.then((rows) => categoriesForScope(rows, 'market'))
			.catch((err: unknown) => {
				marketCategories = null;
				throw err;
			});
	}
	return marketCategories;
}

/** Test/teardown hook; the production path never needs to drop the cache. */
export function resetMarketCategoriesCache(): void {
	marketCategories = null;
}
