<script lang="ts">
    import { goto } from '$app/navigation';

    let query = $state('');
    let open = $state(false);
    let input: HTMLInputElement | null = $state(null);

    // `autofocus` steals focus on page load; focus only once the user opens the box themselves.
    $effect(() => { if (open) input?.focus(); });

    function handleSubmit(e: Event) {
        e.preventDefault();
        const q = query.trim();
        if (!q) return;
        open = false;
        goto(`/search?q=${encodeURIComponent(q)}`);
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.key === 'Escape') {
            open = false;
            query = '';
        }
    }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if !open}
    <button class="search-trigger" onclick={() => open = true} aria-label="Search">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
    </button>
{:else}
    <form class="search-form" onsubmit={handleSubmit}>
        <input
            type="search"
            bind:this={input}
            bind:value={query}
            placeholder="Search posts and users..."
            class="search-input"
        />
        <button type="button" class="search-close" onclick={() => { open = false; query = ''; }} aria-label="Close search">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
        </button>
    </form>
{/if}

<style>
    .search-trigger {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        padding: 0.25rem;
        display: flex;
        align-items: center;
        z-index: 2;
    }

    .search-trigger:hover {
        color: var(--text);
    }

    .search-form {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        z-index: 2;
    }

    .search-input {
        background: var(--bg-surface);
        border: 1px solid var(--accent);
        border-radius: var(--radius, 6px);
        padding: 0.35rem 0.6rem;
        color: var(--text);
        font-size: 0.9rem;
        width: 180px;
        outline: none;
    }

    .search-input::placeholder {
        color: var(--text-muted);
    }

    .search-close {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        display: flex;
        align-items: center;
        padding: 0.15rem;
    }

    .search-close:hover {
        color: var(--text);
    }
</style>
