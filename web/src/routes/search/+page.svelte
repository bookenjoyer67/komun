<script lang="ts">
    import { onMount, untrack } from 'svelte';
    import EmptyState from '$lib/components/ui/EmptyState.svelte';
    import { goto } from '$app/navigation';
    import { getActiveServer, isConnected } from '$lib/stores/server';

    let { data } = $props();
    let q = $derived(data.q || '');

    let posts: any[] = $state([]);
    let users: any[] = $state([]);
    let tab = $state('posts');
    let loading = $state(true);
    let error = $state('');

    // Editable copy seeded once from the URL's `q`; `untrack` keeps typing from being overwritten by the prop.
    let searchQuery = $state(untrack(() => data.q || ''));

    onMount(async () => {
        if (!isConnected()) { goto('/connect'); return; }
        if (!searchQuery) return;
        await Promise.all([
            searchPosts(),
            searchUsers(),
        ]);
        loading = false;
    });

    async function searchPosts() {
        try {
            posts = await fetch(`${getActiveServer()}/api/search?q=${encodeURIComponent(searchQuery)}`).then(r => r.json());
        } catch (e) { }
    }

    async function searchUsers() {
        try {
            users = await fetch(`${getActiveServer()}/api/search/users?q=${encodeURIComponent(searchQuery)}`).then(r => r.json());
        } catch (e) { }
    }

    function handleSearch(e: Event) {
        e.preventDefault();
        if (!searchQuery.trim()) return;
        goto(`/search?q=${encodeURIComponent(searchQuery)}`);
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

<div class="container">
    <header class="search-header">
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

    {#if loading}
        <p class="status">Searching...</p>
    {:else}
        {#if tab === 'posts'}
            {#if posts.length === 0}
                <EmptyState title={`No posts found for "${q}".`} />
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
                            <h3><a href="/p/{post.id}">{post.title}</a></h3>
                            {#if post.body}
                                <p class="post-body">{post.body.slice(0, 200)}{post.body.length > 200 ? '...' : ''}</p>
                            {/if}
                            {#if post.location_name}
                                <span class="location">{post.location_name}</span>
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

    .result-item h3 {
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
</style>
