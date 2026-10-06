<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { isConnected } from '$lib/stores/server';
	import { location as savedLocation } from '$lib/stores/location';
	import MarketCard from '$lib/components/MarketCard.svelte';
	import {
		DEFAULT_RADIUS_KM,
		distanceLabel,
		filtersToQuery,
		ITEM_CONDITIONS,
		listMarketPosts,
		nextRadius,
		parsePriceToCents,
		parseRadiusChoice,
		queryToFilters,
		RADIUS_CHOICES,
		type Centre,
		type MarketFilters,
		type MarketKind,
		type MarketPost
	} from '$lib/api/market';
	import { fetchMarketCategories, type Category } from '$lib/api/categories';

	// The server caps `limit` at 200; asking for more is rejected, not clamped.
	const PAGE_SIZE = 60;
	const MAX_LIMIT = 200;

	let categories = $state<Category[]>([]);
	let posts = $state<MarketPost[]>([]);
	let loading = $state(true);
	let loadingMore = $state(false);
	let error = $state('');
	let limit = $state(PAGE_SIZE);
	let connected = $state(false);

	// URL is the single source of truth for filters, so a filtered view is linkable and Back works.
	let filters = $derived(queryToFilters($page.url.searchParams));

	let centre = $derived<Centre>({ lat: $savedLocation.lat, lon: $savedLocation.lon });
	let hasCentre = $derived(centre.lat != null && centre.lon != null);
	let radius = $derived(filters.radius_km ?? DEFAULT_RADIUS_KM);
	let wider = $derived(nextRadius(radius));

	// Text-input drafts, kept apart from the URL so typing does not rewrite history on every keystroke.
	let searchText = $state('');
	let minPriceText = $state('');
	let maxPriceText = $state('');

	// Guards the effect from re-loading what it just applied; `null` (not `''`) so a bare `/market` still loads.
	let lastLoaded = $state<string | null>(null);

	onMount(async () => {
		if (!isConnected()) {
			goto('/connect');
			return;
		}
		connected = true;
		try {
			categories = await fetchMarketCategories();
		} catch {
			// Browsing must not fail if the taxonomy request did; the category select falls back to the raw slug.
			categories = [];
		}
	});

	$effect(() => {
		const query = $page.url.searchParams.toString();
		const activeCentre = centre;
		// A moved location changes the results as much as a changed filter does.
		const key = `${query}|${activeCentre.lat},${activeCentre.lon}`;
		// Tracked, not untracked: the effect must run once `connected` flips true even if it ran first.
		if (!connected) return;
		if (key === untrack(() => lastLoaded)) return;

		const parsed = queryToFilters(query);
		lastLoaded = key;
		searchText = parsed.q ?? '';
		minPriceText = parsed.min_price_cents != null ? String(parsed.min_price_cents / 100) : '';
		maxPriceText = parsed.max_price_cents != null ? String(parsed.max_price_cents / 100) : '';
		limit = PAGE_SIZE;
		void runLoad(parsed, PAGE_SIZE, activeCentre);
	});

	async function runLoad(active: MarketFilters, size: number, at: Centre) {
		loading = true;
		error = '';
		try {
			posts = await listMarketPosts({ ...active, limit: size }, at);
		} catch (e) {
			// A rejected filter is a server-named error, not an empty result; "no listings" would hide the mistake.
			error = e instanceof Error ? e.message : 'Failed to load the marketplace';
			posts = [];
		} finally {
			loading = false;
		}
	}

	function applyFilters(partial: Partial<MarketFilters>) {
		const query = filtersToQuery({ ...filters, ...partial });
		goto(query ? `/market?${query}` : '/market', { keepFocus: true, noScroll: true });
	}

	async function loadMore() {
		const nextLimit = Math.min(limit + PAGE_SIZE, MAX_LIMIT);
		if (nextLimit <= limit) return;
		loadingMore = true;
		error = '';
		try {
			posts = await listMarketPosts({ ...filters, limit: nextLimit }, centre);
			limit = nextLimit;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load more listings';
		} finally {
			loadingMore = false;
		}
	}

	function setKind(kind: MarketKind | '') {
		applyFilters({ kind: kind === '' ? undefined : kind });
	}

	function onCategory(e: Event) {
		const value = (e.currentTarget as HTMLSelectElement).value;
		applyFilters({ category: value || undefined });
	}

	function onCondition(e: Event) {
		const value = (e.currentTarget as HTMLSelectElement).value;
		applyFilters({
			item_condition: (value || undefined) as MarketFilters['item_condition']
		});
	}

	// The default stays out of the URL, so an untouched view links as a bare `/market`.
	function setRadius(choice: MarketFilters['radius_km']) {
		applyFilters({ radius_km: choice === DEFAULT_RADIUS_KM ? undefined : choice });
	}

	function onRadius(e: Event) {
		setRadius(parseRadiusChoice((e.currentTarget as HTMLSelectElement).value));
	}

	function widen() {
		if (wider !== null) setRadius(wider);
	}

	function onSearch(e: Event) {
		e.preventDefault();
		applyFilters({ q: searchText.trim() || undefined });
	}

	function onPriceCommit() {
		applyFilters({
			min_price_cents: parsePriceToCents(minPriceText) ?? undefined,
			max_price_cents: parsePriceToCents(maxPriceText) ?? undefined
		});
	}

	function clearFilters() {
		searchText = '';
		minPriceText = '';
		maxPriceText = '';
		goto('/market', { keepFocus: true, noScroll: true });
	}

	let hasFilters = $derived(
		Boolean(
			filters.kind ||
				filters.category ||
				filters.q ||
				filters.item_condition ||
				filters.min_price_cents != null ||
				filters.max_price_cents != null ||
				filters.radius_km != null ||
				filters.sort
		)
	);
	// At the cap, newer posts may exist beyond the truncation, so say so rather than imply the end.
	let capped = $derived(posts.length >= limit && limit >= MAX_LIMIT);
</script>

<svelte:head>
	<title>Market — Komun</title>
	<meta name="description" content="Listings and wanted ads on Komun" />
</svelte:head>

<div class="container">
	<header class="page-header">
		<div>
			<h1>Market</h1>
			<p class="subtitle">Things for sale, and things wanted.</p>
		</div>
		<a href="/market/new" class="btn-primary">Post</a>
	</header>

	<form class="search-bar" onsubmit={onSearch}>
		<input
			type="search"
			bind:value={searchText}
			placeholder="Search listings and wanted ads..."
			aria-label="Search the market"
		/>
		<button type="submit" class="search-btn">Search</button>
	</form>

	<div class="filters">
		<div class="kind-toggle" role="group" aria-label="Kind">
			<button type="button" class:active={!filters.kind} onclick={() => setKind('')}>Both</button>
			<button
				type="button"
				class:active={filters.kind === 'listing'}
				onclick={() => setKind('listing')}
			>
				Listings
			</button>
			<button type="button" class:active={filters.kind === 'want'} onclick={() => setKind('want')}>
				Wanted
			</button>
		</div>

		<label>
			<span>Category</span>
			<select value={filters.category ?? ''} onchange={onCategory}>
				<option value="">All categories</option>
				{#each categories as category}
					<option value={category.slug}>{category.label}</option>
				{/each}
			</select>
		</label>

		<label>
			<span>Condition</span>
			<select value={filters.item_condition ?? ''} onchange={onCondition}>
				<option value="">Any condition</option>
				{#each ITEM_CONDITIONS as condition}
					<option value={condition.value}>{condition.label}</option>
				{/each}
			</select>
		</label>

		<label>
			<span>Distance</span>
			<select value={String(radius)} onchange={onRadius} disabled={!hasCentre}>
				{#each RADIUS_CHOICES as choice}
					<option value={String(choice.value)}>{choice.label}</option>
				{/each}
			</select>
		</label>

		{#if hasCentre}
			<button
				type="button"
				class="widen"
				disabled={wider === null || loading}
				aria-busy={loading}
				onclick={widen}
			>
				Widen the area
			</button>
		{/if}

		<label class="price-field">
			<span>Min price</span>
			<input type="number" min="0" step="0.01" bind:value={minPriceText} onchange={onPriceCommit} />
		</label>

		<label class="price-field">
			<span>Max price</span>
			<input type="number" min="0" step="0.01" bind:value={maxPriceText} onchange={onPriceCommit} />
		</label>

		{#if hasFilters}
			<button type="button" class="clear-filters" onclick={clearFilters}>Clear filters</button>
		{/if}
	</div>

	{#if !hasCentre}
		<p class="hint">Set your location to filter by distance.</p>
	{/if}

	{#if error}
		<p class="status error" role="alert">{error}</p>
	{:else if loading}
		<p class="status">Loading...</p>
	{:else if posts.length === 0}
		<p class="status">
			{#if hasCentre && radius !== 'any'}
				No listings within {radius} km match these filters yet.
			{:else}
				No listings match these filters yet.
			{/if}
		</p>
	{:else}
		<div class="market-grid">
			{#each posts as post (post.id)}
				<div class="market-cell">
					<MarketCard {post} />
					{#if distanceLabel(post.distance_km)}
						<p class="distance">{distanceLabel(post.distance_km)}</p>
					{/if}
				</div>
			{/each}
		</div>

		{#if capped}
			<p class="status">Reached the server's result limit. Narrow the filters to see more.</p>
		{:else if posts.length >= limit}
			<div class="more">
				<button type="button" class="load-more" disabled={loadingMore} onclick={loadMore}>
					{loadingMore ? 'Loading...' : 'Load more'}
				</button>
			</div>
		{/if}
	{/if}
</div>

<style>
	.page-header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		margin-bottom: 1.25rem;
	}

	h1 {
		font-size: 1.5rem;
	}

	.subtitle {
		color: var(--text-muted);
		font-size: 0.85rem;
		margin-top: 0.15rem;
	}

	.btn-primary {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.5rem 1rem;
		border-radius: var(--radius);
		font-weight: 600;
		font-size: 0.9rem;
		text-decoration: none;
	}

	.search-bar {
		display: flex;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}

	.search-bar input {
		flex: 1;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.6rem 0.75rem;
		color: var(--text);
		font-size: 0.9rem;
	}

	.search-bar input:focus {
		outline: none;
		border-color: var(--accent);
	}

	.search-btn {
		background: var(--bg-elevated);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.6rem 1rem;
		font-size: 0.85rem;
	}

	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: flex-end;
		margin-bottom: 1.5rem;
	}

	.kind-toggle {
		display: flex;
		gap: 0.35rem;
	}

	.kind-toggle button {
		background: var(--bg-surface);
		color: var(--text-muted);
		padding: 0.4rem 0.8rem;
		border-radius: var(--radius);
		font-size: 0.85rem;
		border: 1px solid var(--border);
	}

	.kind-toggle button.active {
		background: var(--accent-soft);
		color: var(--accent);
		border-color: var(--accent);
	}

	.filters label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.filters label span {
		font-size: 0.72rem;
		font-weight: 600;
		color: var(--text-muted);
	}

	.filters select,
	.filters input {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.4rem 0.55rem;
		color: var(--text);
		font-size: 0.85rem;
	}

	.filters select:focus,
	.filters input:focus {
		outline: none;
		border-color: var(--accent);
	}

	.filters select:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.price-field input {
		width: 7rem;
	}

	.widen {
		background: var(--bg-elevated);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: var(--space-2) var(--space-3);
		font-size: var(--text-sm);
		font-weight: 600;
		transition: border-color var(--transition-fast);
	}

	.widen:hover:not(:disabled) {
		border-color: var(--accent);
	}

	.widen:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}

	.widen:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.widen[aria-busy='true'] {
		cursor: progress;
	}

	.clear-filters {
		background: none;
		border: none;
		color: var(--accent);
		font-size: 0.8rem;
		font-weight: 600;
		text-decoration: underline;
		padding: 0.4rem 0;
		min-height: unset;
	}

	.hint {
		color: var(--text-muted);
		font-size: var(--text-sm);
		margin-bottom: var(--space-4);
	}

	.market-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 0.9rem;
	}

	.market-cell {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}

	.distance {
		color: var(--text-muted);
		font-size: var(--text-xs);
	}

	.more {
		display: flex;
		justify-content: center;
		margin-top: 1.5rem;
	}

	.load-more {
		background: var(--bg-elevated);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: var(--radius-full);
		padding: 0.5rem 1.5rem;
		font-weight: 600;
	}

	.load-more:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.status {
		text-align: center;
		color: var(--text-muted);
		padding: 3rem 0;
	}

	.error {
		color: var(--critical);
	}

	@media (max-width: 480px) {
		.page-header {
			flex-direction: column;
			gap: 0.5rem;
		}

		.filters {
			flex-direction: column;
			align-items: stretch;
		}
	}
</style>
