<script lang="ts">
    import { onMount } from 'svelte';
    import { goto } from '$app/navigation';
    import { getActiveServer, isConnected } from '$lib/stores/server';
    import { getActiveAuth, getToken } from '$lib/stores/auth';
    import { api } from '$lib/api/client';
    import { safeHttpUrl } from '$lib/safeUrl';

    let { data } = $props();

    let profile = $derived(data.profile);
    let isOwnProfile = $derived(data.isOwnProfile);
    let endorsements = $derived(data.endorsements);
    let error = $derived(data.error);
    let copied = $state(false);
    let endorsing = $state(false);
    // Written optimistically by the endorse handlers, so derive (not seed $state from the prop) to stay in step with a fresh load.
    let endorsed = $derived(data.hasEndorsed);
    let endCount = $derived(data.endorsements?.count || 0);
    let endorseError = $state('');

    // profile_json is owner-written through PUT /api/auth/me, so a link renders only with an http(s) href; any other link is dropped.
    const safeLinks = $derived(
        (Array.isArray(profile?.profile_json?.links) ? profile.profile_json.links : []).flatMap(
            (link: { label?: unknown; url?: unknown } | null) => {
                const href = safeHttpUrl(link?.url);
                return href ? [{ label: link?.label, href }] : [];
            }
        )
    );

    function copyPublicKey() {
        if (!profile?.public_key) return;
        navigator.clipboard.writeText(profile.public_key);
        copied = true;
        setTimeout(() => copied = false, 1500);
    }

    async function handleEndorse() {
        if (!profile) return;
        endorseError = '';
        endorsing = true;
        try {
            await api.endorsements.endorse(profile.id);
            endorsed = true;
            endCount++;
            endorsements = { ...endorsements, count: endCount, endorsements: [...(endorsements?.endorsements || []), { endorser_id: getActiveAuth()?.userId, endorser_name: 'You', note: null, created_at: new Date().toISOString() }] };
        } catch (e: any) {
            endorseError = e.message || 'Failed to endorse';
        }
        endorsing = false;
    }

    async function handleUnendorse() {
        if (!profile) return;
        endorseError = '';
        endorsing = true;
        try {
            await api.endorsements.unendorse(profile.id);
            endorsed = false;
            endCount = Math.max(0, endCount - 1);
            const myId = getActiveAuth()?.userId;
            endorsements = { ...endorsements, count: endCount, endorsements: (endorsements?.endorsements || []).filter((e: any) => e.endorser_id !== myId) };
        } catch (e: any) {
            endorseError = e.message || 'Failed to remove endorsement';
        }
        endorsing = false;
    }

    function relativeTime(iso: string): string {
        const d = new Date(iso);
        const now = Date.now();
        const diff = now - d.getTime();
        const mins = Math.floor(diff / 60000);
        if (mins < 1) return 'just now';
        if (mins < 60) return `${mins}m ago`;
        const hrs = Math.floor(mins / 60);
        if (hrs < 24) return `${hrs}h ago`;
        const days = Math.floor(hrs / 24);
        if (days < 30) return `${days}d ago`;
        const months = Math.floor(days / 30);
        return `${months}mo ago`;
    }
</script>

{#if error}
    <div class="container">
        <div class="error-state">
            <h2>{error}</h2>
            <a href="/">Back to home</a>
        </div>
    </div>
{:else if profile}
    <div class="container">
        {#if isOwnProfile}
            <div class="own-banner">This is your profile — <a href="/account">edit</a></div>
        {/if}

        <div class="profile-header">
            {#if profile.avatar_url}
                <img src={profile.avatar_url} alt={profile.display_name} class="avatar" />
            {:else}
                <div class="avatar-placeholder">{profile.display_name?.[0]?.toUpperCase() || '?'}</div>
            {/if}

            <h1 class="display-name">{profile.display_name}</h1>

            {#if profile.role === 'superadmin'}
                <span class="role-badge superadmin">superadmin</span>
            {:else if profile.role === 'admin'}
                <span class="role-badge admin">admin</span>
            {/if}
        </div>

        <div
            class="rating-badge"
            aria-label={profile.rating_count > 0
                ? `Rated ${profile.rating_avg} out of 5 from ${profile.rating_count} reviews`
                : 'No reviews yet'}
        >
            {#if profile.rating_count > 0}
                <span class="rating-stars" aria-hidden="true">
                    {#each [1, 2, 3, 4, 5] as star (star)}{star <= Math.round(profile.rating_avg) ? '★' : '☆'}{/each}
                </span>
                <span class="rating-value">{profile.rating_avg}</span>
                <span class="rating-count"
                    >({profile.rating_count}
                    {profile.rating_count === 1 ? 'review' : 'reviews'})</span
                >
            {:else}
                <span class="rating-none">No reviews yet</span>
            {/if}
        </div>

        {#if profile.bio}
            <div class="bio-section">
                <p>{profile.bio}</p>
            </div>
        {/if}

        <div class="stats-row">
            <div class="stat">
                <span class="stat-value">{profile.post_count}</span>
                <span class="stat-label">posts</span>
            </div>
            <div class="stat">
                <span class="stat-value">{profile.verified_post_count}</span>
                <span class="stat-label">verified</span>
            </div>
            <div class="stat">
                <span class="stat-value">{endCount}</span>
                <span class="stat-label">endorsements</span>
            </div>
        </div>

        {#if !isOwnProfile && getActiveAuth()}
            <div class="endorse-section">
                {#if endorsed}
                    <button class="endorse-btn endorsed" onclick={handleUnendorse} disabled={endorsing}>
                        {endorsing ? '...' : 'Endorsed ✓ (click to remove)'}
                    </button>
                {:else}
                    <button class="endorse-btn" onclick={handleEndorse} disabled={endorsing}>
                        {endorsing ? '...' : 'Endorse'}
                    </button>
                {/if}
                {#if endorseError}
                    <p class="endorse-error">{endorseError}</p>
                {/if}
            </div>
        {/if}

        <div class="meta-section">
            <div class="meta-row">
                <span class="meta-label">Joined</span>
                <span class="meta-value">{profile.joined_at ? relativeTime(profile.joined_at) : '—'}</span>
            </div>
            <div class="meta-row">
                <span class="meta-label">Last seen</span>
                <span class="meta-value">{profile.last_seen ? relativeTime(profile.last_seen) : '—'}</span>
            </div>
        </div>

        {#if endorsements?.endorsements?.length}
            <div class="endorsements-section">
                <h3>Endorsements ({endorsements.count})</h3>
                <ul class="endorsements-list">
                    {#each endorsements.endorsements as e}
                        <li class="endorsement-item">
                            <a href="/users/{e.endorser_id}" class="endorser-name">{e.endorser_name}</a>
                            {#if e.note}
                                <p class="endorsement-note">"{e.note}"</p>
                            {/if}
                            <span class="endorsement-time">{relativeTime(e.created_at)}</span>
                        </li>
                    {/each}
                </ul>
            </div>
        {/if}

        <div class="key-section">
            <span class="meta-label">Public key</span>
            <div class="key-display">
                <code>{profile.public_key?.slice(0, 24)}...{profile.public_key?.slice(-12)}</code>
                <button class="copy-btn" onclick={copyPublicKey}>
                    {copied ? 'Copied!' : 'Copy'}
                </button>
            </div>
        </div>

        {#if safeLinks.length}
            <div class="links-section">
                <h3>Links</h3>
                {#each safeLinks as link}
                    <a href={link.href} target="_blank" rel="noopener" class="profile-link">{link.label || link.href}</a>
                {/each}
            </div>
        {/if}
    </div>
{:else}
    <div class="container">
        <p>Loading...</p>
    </div>
{/if}

<style>
    .container {
        max-width: 640px;
        margin: 0 auto;
        padding: 2rem 1rem;
    }

    .error-state {
        text-align: center;
        padding: 4rem 1rem;
    }

    .own-banner {
        background: var(--accent-soft, rgba(30, 255, 157, 0.1));
        border: 1px solid var(--accent);
        border-radius: var(--radius);
        padding: 0.5rem 1rem;
        text-align: center;
        margin-bottom: 1.5rem;
        font-size: 0.9rem;
    }

    .own-banner a {
        color: var(--accent);
        font-weight: 600;
    }

    .profile-header {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 1.5rem;
    }

    .avatar {
        width: 128px;
        height: 128px;
        border-radius: 50%;
        object-fit: cover;
        border: 3px solid var(--border);
    }

    .avatar-placeholder {
        width: 128px;
        height: 128px;
        border-radius: 50%;
        background: var(--bg-elevated);
        border: 3px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 3rem;
        font-weight: 700;
        color: var(--text-muted);
    }

    .display-name {
        font-size: 1.6rem;
        margin: 0;
    }

    .role-badge {
        font-size: 0.7rem;
        padding: 0.15rem 0.5rem;
        border-radius: var(--radius-sm);
        font-weight: 600;
        text-transform: uppercase;
    }

    .role-badge.superadmin {
        background: var(--critical-soft);
        color: var(--critical);
        border: 1px solid var(--critical);
    }

    .role-badge.admin {
        background: var(--accent-soft, rgba(30, 255, 157, 0.1));
        color: var(--accent);
        border: 1px solid var(--accent);
    }

    .rating-badge {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 0.35rem;
        margin-bottom: 1.25rem;
        font-size: 0.9rem;
    }

    .rating-stars {
        color: var(--warning);
        letter-spacing: 0.1em;
    }

    .rating-value {
        font-weight: 700;
    }

    .rating-count,
    .rating-none {
        color: var(--text-muted);
        font-size: 0.8rem;
    }

    .bio-section {
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 1rem;
        margin-bottom: 1.5rem;
    }

    .bio-section p {
        margin: 0;
        color: var(--text-muted);
        line-height: 1.6;
    }

    .stats-row {
        display: flex;
        gap: 1rem;
        margin-bottom: 0.75rem;
    }

    .stat {
        flex: 1;
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.75rem;
        text-align: center;
    }

    .stat-value {
        display: block;
        font-size: 1.5rem;
        font-weight: 700;
    }

    .stat-label {
        display: block;
        font-size: 0.75rem;
        color: var(--text-muted);
        text-transform: uppercase;
        margin-top: 0.2rem;
    }

    .endorse-section {
        margin-bottom: 1rem;
        text-align: center;
    }

    .endorse-btn {
        background: var(--bg-elevated);
        color: var(--text);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.5rem 1.5rem;
        font-weight: 600;
        font-size: 0.9rem;
        cursor: pointer;
        transition: border-color var(--transition-fast), background var(--transition-fast);
    }

    .endorse-btn:hover:not(:disabled) {
        border-color: var(--accent);
        background: var(--accent-soft, rgba(30, 255, 157, 0.1));
    }

    .endorse-btn.endorsed {
        border-color: var(--success);
        color: var(--success);
    }

    .endorse-btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .endorse-error {
        color: var(--critical);
        font-size: 0.8rem;
        margin-top: 0.3rem;
    }

    .meta-section {
        margin-bottom: 1.5rem;
    }

    .meta-row {
        display: flex;
        justify-content: space-between;
        padding: 0.5rem 0;
        border-bottom: 1px solid var(--border);
    }

    .meta-label {
        color: var(--text-muted);
        font-size: 0.85rem;
    }

    .meta-value {
        font-size: 0.85rem;
    }

    .endorsements-section {
        margin-bottom: 1.5rem;
    }

    .endorsements-section h3 {
        font-size: 0.95rem;
        margin-bottom: 0.5rem;
    }

    .endorsements-list {
        list-style: none;
    }

    .endorsement-item {
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.6rem 0.8rem;
        margin-bottom: 0.4rem;
    }

    .endorser-name {
        font-weight: 600;
        font-size: 0.9rem;
        color: var(--accent);
    }

    .endorsement-note {
        color: var(--text-muted);
        font-size: 0.8rem;
        font-style: italic;
        margin-top: 0.15rem;
    }

    .endorsement-time {
        color: var(--text-muted);
        font-size: 0.7rem;
    }

    .key-section {
        margin-bottom: 1.5rem;
    }

    .key-display {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        background: var(--bg-surface);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 0.5rem 0.75rem;
        margin-top: 0.3rem;
    }

    .key-display code {
        font-size: 0.8rem;
        color: var(--text-muted);
        flex: 1;
        word-break: break-all;
    }

    .copy-btn {
        background: var(--bg-elevated);
        color: var(--text);
        padding: 0.25rem 0.5rem;
        border-radius: var(--radius);
        font-size: 0.75rem;
        border: 1px solid var(--border);
    }

    .links-section {
        margin-bottom: 1.5rem;
    }

    .links-section h3 {
        font-size: 1rem;
        margin-bottom: 0.5rem;
    }

    .profile-link {
        display: block;
        color: var(--accent);
        padding: 0.3rem 0;
    }
</style>
