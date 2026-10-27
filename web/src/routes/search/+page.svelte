<script lang="ts">
    import { onMount, untrack } from 'svelte';
    import EmptyState from '$lib/components/ui/EmptyState.svelte';
    import { goto } from '$app/navigation';
    import { getActiveServer, isConnected } from '$lib/stores/server';
    import { getToken, isAuthenticated } from '$lib/stores/auth';
    import { location as savedLocation } from '$lib/stores/location';
    import { api } from '$lib/api/client';
    import {
        DEFAULT_RADIUS_KM,
        distanceLabel,
        nextRadius,
        parseRadiusChoice,
        RADIUS_CHOICES,
        radiusParams,
        type Centre,
        type RadiusChoice
    } from '$lib/api/market';

    let { data } = $props();
    let q = $derived(data.q || '');

    let posts: any[] = $state([]);
    let users: any[] = $state([]);
    let tab = $state('posts');
    let searchingPosts = $state(false);
    let error = $state('');

    let radius = $state<RadiusChoice>(DEFAULT_RADIUS_KM);
    let centre = $derived<Centre>({ lat: $savedLocation.lat, lon: $savedLocation.lon });
    let hasCentre = $derived(centre.lat != null && centre.lon != null);
    let centreKey = $derived(`${centre.lat},${centre.lon}`);
    let wider = $derived(nextRadius(radius));

    // Editable copy seeded once from the URL's `q`; `untrack` keeps typing from being overwritten by the prop.
    let searchQuery = $state(untrack(() => data.q || ''));
    // The submitted `q`, never the draft: unsubmitted text has had no search, so it has no result to report.
    // `q=''` returns the unfiltered feed, so an empty query is never sent and never shows "Searching...".
    let hasQuery = $derived(q.trim() !== '');
    let loading = $state(untrack(() => hasQuery));

    let connected = $state(false);
    let signedIn = $state(false);
    // Guards the effect from repeating the search onMount just ran.
    let lastCentre = $state<string | null>(null);
    // Guards the query effect from repeating the search onMount just ran.
    let lastQ = $state<string | null>(null);

    // Built by `radiusParams`, so a saved search carries the same coarse cell the live search sends.
    let saveBody = $derived<Record<string, string | number>>({
        q: q.trim(),
        ...Object.fromEntries(
            Object.entries(radiusParams(centre, radius)).map(([key, value]) => [key, Number(value)])
        )
    });
    // An outcome belongs to the search it was for: a new query, radius or centre is a new save.
    let saveKey = $derived(JSON.stringify(saveBody));
    let saving = $state(false);
    let saveResult = $state<{ key: string; ok: boolean; message: string } | null>(null);
    let savedHere = $derived(saveResult?.ok === true && saveResult.key === saveKey);
    let saveError = $derived(
        saveResult && !saveResult.ok && saveResult.key === saveKey ? saveResult.message : ''
    );

    onMount(async () => {
        if (!isConnected()) { goto('/connect'); return; }
        lastCentre = centreKey;
        lastQ = q;
        connected = true;
        signedIn = isAuthenticated();
        if (!hasQuery) return;
        await searchAll();
    });

    $effect(() => {
        // A moved location changes the results as much as a new radius does.
        const key = centreKey;
        if (!connected) return;
        if (key === untrack(() => lastCentre)) return;
        lastCentre = key;
        untrack(() => {
            if (hasQuery) void searchPosts();
        });
    });

    // A `goto` to a new `?q=` reuses this component, so onMount never sees the new query.
    // `pre`, so the new `q` is never rendered with `loading` still false, which would report the old results under it.
    $effect.pre(() => {
        const submitted = q;
        if (!connected) return;
        if (submitted === untrack(() => lastQ)) return;
        lastQ = submitted;
        untrack(() => {
            loading = hasQuery;
            if (hasQuery) void searchAll();
        });
    });

    // `/api/posts`, not `/api/search`: only the feed carries coordinates, so only it can filter by distance.
    async function searchPosts() {
        searchingPosts = true;
        error = '';
        try {
            posts = await api.posts.list({ q, ...radiusParams(centre, radius) });
        } catch (e) {
            // A refused search is a named error, not "no posts found".
            error = e instanceof Error ? e.message : 'Search failed';
            posts = [];
        } finally {
            searchingPosts = false;
        }
    }

    async function searchUsers() {
        try {
            users = await fetch(`${getActiveServer()}/api/search/users?q=${encodeURIComponent(q)}`).then(r => r.json());
        } catch (e) { }
    }

    // A superseded search that finishes first must not end the loading state of the query that replaced it.
    async function searchAll() {
        const searched = q;
        await Promise.all([
            searchPosts(),
            searchUsers(),
        ]);
        if (q === searched) loading = false;
    }

    function handleSearch(e: Event) {
        e.preventDefault();
        if (!searchQuery.trim()) return;
        goto(`/search?q=${encodeURIComponent(searchQuery)}`);
    }

    function onRadius(e: Event) {
        radius = parseRadiusChoice((e.currentTarget as HTMLSelectElement).value);
        if (hasQuery) void searchPosts();
    }

    function widen() {
        if (wider === null) return;
        radius = wider;
        if (hasQuery) void searchPosts();
    }

    async function saveSearch() {
        const key = saveKey;
        const body = saveBody;
        saving = true;
        try {
            const res = await fetch(`${getActiveServer()}/api/me/saved-searches`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${getToken()}` },
                body: JSON.stringify(body),
            });
            if (!res.ok) {
                const data = await res.json().catch(() => ({}));
                throw new Error(data.error || 'Could not save this search.');
            }
            saveResult = { key, ok: true, message: '' };
        } catch (e) {
            saveResult = {
                key,
                ok: false,
                message: e instanceof Error ? e.message : 'Could not save this search.'
            };
        } finally {
            saving = false;
        }
    }

    function kindBadge(kind: string): string {
        const m: Record<string, string> = { need: 'Need', offer: 'Offer', resource: 'Resource', listing: 'Listing', want: 'Want' };
        return m[kind] || kind;
    }

    // The palette carries two severity levels, not three; high and medium share --warning.
    function urgencyColor(urgency: string): string {
        const m: Record<string, string> = {
            critical: 'var(--critical)',
            high: 'var(--warning)',
            medium: 'var(--warning)',
            low: 'var(--text-muted)'
        };
        return m[urgency] || 'var(--text-muted)';
    }
</script>

<svelte:head>
    <title>Search — Komun</title>
</svelte:head>

<div class="container">
    <header class="search-header">
        <h1>Search</h1>
        <form onsubmit={handleSearch} class="search-bar">
            <input
                type="search"
                bind:value={searchQuery}
                placeholder="Search posts and users..."
                class="search-input"
            />
        </form>
    </header>

    <div class="tabs">
        <button class="tab" class:active={tab === 'posts'} onclick={() => tab = 'posts'}>
            Posts ({posts.length})
        </button>
        <button class="tab" class:active={tab === 'users'} onclick={() => tab = 'users'}>
            Users ({users.length})
        </button>
    </div>

    {#if tab === 'posts'}
        <div class="radius">
            <label>
                <span>Distance</span>
                <select value={String(radius)} onchange={onRadius} disabled={!hasCentre || searchingPosts}>
                    {#each RADIUS_CHOICES as choice}
                        <option value={String(choice.value)}>{choice.label}</option>
                    {/each}
                </select>
            </label>
            {#if hasCentre}
                <button
                    type="button"
                    class="widen"
                    disabled={wider === null || searchingPosts}
                    aria-busy={searchingPosts}
                    onclick={widen}
                >
                    Widen the area
                </button>
            {:else}
                <p class="hint">Set your location to filter by distance.</p>
            {/if}
            {#if hasQuery && signedIn}
                <button
                    type="button"
                    class="save-search"
                    disabled={saving || savedHere}
                    aria-busy={saving}
                    onclick={saveSearch}
                >
                    Save this search
                </button>
            {/if}
        </div>
        {#if hasQuery && signedIn}
            {#if savedHere}
                <p class="save-note" role="status">
                    Search saved. New matches arrive as a daily digest; manage it on your <a href="/account">account page</a>.
                </p>
            {:else if saveError}
                <p class="save-error" role="alert">{saveError}</p>
            {/if}
        {/if}
    {/if}

    {#if loading}
        <p class="status">Searching...</p>
    {:else if !hasQuery}
        <p class="status">Type a word to search posts and users.</p>
    {:else}
        {#if tab === 'posts'}
            {#if error}
                <p class="status error" role="alert">{error}</p>
            {:else if posts.length === 0}
                <EmptyState
                    title={hasCentre && radius !== 'any'
                        ? `No posts found for "${q}" within ${radius} km.`
                        : `No posts found for "${q}".`}
                />
            {:else}
                <ul class="results-list">
                    {#each posts as post}
                        <li class="result-item">
                            <div class="post-header">
                                <span class="kind-badge kind-{post.kind}">{kindBadge(post.kind)}</span>
                                {#if post.urgency}
                                    <span class="urgency" style="color: {urgencyColor(post.urgency)}">● {post.urgency}</span>
                                {/if}
                            </div>
                            <h2><a href="/p/{post.id}">{post.title}</a></h2>
                            {#if post.body}
                                <p class="post-body">{post.body.slice(0, 200)}{post.body.length > 200 ? '...' : ''}</p>
                            {/if}
                            {#if post.location_name || distanceLabel(post.distance_km)}
                                <span class="location">
                                    {post.location_name ?? ''}{post.location_name && distanceLabel(post.distance_km) ? ' · ' : ''}{distanceLabel(post.distance_km)}
                                </span>
                            {/if}
                        </li>
                    {/each}
                </ul>
            {/if}
        {:else}
            {#if users.length === 0}
                <EmptyState title={`No users found for "${q}".`} />
            {:else}
                <ul class="results-list">
                    {#each users as u}
                        <li class="result-item">
                            <a href="/users/{u.id}"><strong>{u.display_name}</strong></a>
                            <span class="user-meta">
                                {#if u.endorsement_count > 0}
                                    {u.endorsement_count} end{u.endorsement_count === 1 ? 'orsement' : 'orsements'}
                                {/if}
                            </span>
                        </li>
                    {/each}
                </ul>
            {/if}
        {/if}
    {/if}
</div>

<style>
    .container {
        max-width: 640px;
        margin: 0 auto;
        padding: 1rem;
    }

    .search-header {
        margin-bottom: 1rem;
    }

    .search-header h1 {
        font-size: var(--text-2xl);
        margin-bottom: var(--space-3);
    }

    .search-input {
        width: 100%;
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.7rem 1rem;
        color: var(--text);
        font-size: 1rem;
        outline: none;
    }

    .search-input:focus {
        border-color: var(--accent);
    }

    .tabs {
        display: flex;
        gap: 0.5rem;
        margin-bottom: 1.5rem;
        border-bottom: 1px solid var(--border);
        padding-bottom: 0.5rem;
    }

    .tab {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: 0.9rem;
        padding: 0.3rem 0.6rem;
        cursor: pointer;
        border-radius: var(--radius-sm);
    }

    .tab.active {
        color: var(--text);
        background: var(--bg-surface);
    }

    .radius {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-3);
        align-items: flex-end;
        margin-bottom: var(--space-4);
    }

    .radius label {
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
    }

    .radius label span {
        font-size: var(--text-xs);
        font-weight: 600;
        color: var(--text-muted);
    }

    .radius select {
        width: auto;
        padding: var(--space-2) var(--space-3);
        font-size: var(--text-sm);
    }

    .radius select:disabled {
        opacity: 0.6;
        cursor: not-allowed;
    }

    .widen,
    .save-search {
        background: var(--bg-elevated);
        color: var(--text);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-2) var(--space-3);
        font-size: var(--text-sm);
        font-weight: 600;
        transition: border-color var(--transition-fast);
    }

    .widen:hover:not(:disabled),
    .save-search:hover:not(:disabled) {
        border-color: var(--accent);
    }

    .widen:focus-visible,
    .save-search:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 2px;
    }

    .widen:disabled,
    .save-search:disabled {
        opacity: 0.6;
        cursor: not-allowed;
    }

    .widen[aria-busy='true'],
    .save-search[aria-busy='true'] {
        cursor: progress;
    }

    .save-search {
        margin-left: auto;
    }

    .save-note,
    .save-error {
        font-size: var(--text-sm);
        margin-bottom: var(--space-4);
    }

    .save-note {
        color: var(--text-muted);
    }

    .save-note a {
        color: var(--accent);
        font-weight: 600;
    }

    .save-error {
        color: var(--critical);
    }

    .hint {
        color: var(--text-muted);
        font-size: var(--text-sm);
    }

    .results-list {
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .result-item {
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.75rem;
    }

    .result-item h2 {
        font-size: 1rem;
        margin: 0.25rem 0;
    }

    .result-item a {
        color: var(--accent);
        font-weight: 600;
    }

    .post-header {
        display: flex;
        gap: 0.5rem;
        align-items: center;
        margin-top: 0.15rem;
    }

    .kind-badge {
        font-size: 0.65rem;
        padding: 0.1rem 0.4rem;
        border-radius: var(--radius-sm);
        font-weight: 700;
        text-transform: uppercase;
    }

    .kind-need { background: var(--critical-soft); color: var(--critical); }
    .kind-offer { background: var(--success-soft); color: var(--success); }
    .kind-resource { background: var(--accent-soft); color: var(--accent); }

    .urgency {
        font-size: 0.7rem;
        text-transform: uppercase;
    }

    .post-body {
        font-size: 0.85rem;
        color: var(--text-muted);
        margin-top: 0.25rem;
        line-height: 1.4;
    }

    .location {
        font-size: 0.75rem;
        color: var(--text-muted);
    }

    .user-meta {
        font-size: 0.75rem;
        color: var(--text-muted);
        margin-left: 0.5rem;
    }

    .status {
        text-align: center;
        color: var(--text-muted);
        padding: 3rem 0;
    }

    .error {
        color: var(--critical);
    }
</style>
