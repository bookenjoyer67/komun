<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import LocationBar from '$lib/components/LocationBar.svelte';
	import AidCard from '$lib/components/AidCard.svelte';
	import { location, hasLocation, getLocation } from '$lib/stores/location';
	import { isConnected, connectToServer } from '$lib/stores/server';
	import { discoverNearbyServers, fetchFromServers, type AggregatedPost, type NearbyServer } from '$lib/api/discovery';

	let posts: AggregatedPost[] = $state([]);
	let servers: NearbyServer[] = $state([]);
	let loading = $state(false);
	let searched = $state(false);
	let filter = $state('all');

	onMount(() => {
		if (hasLocation()) {
			loadFeed();
		}
	});

	async function loadFeed() {
		loading = true;
		searched = true;
		try {
			servers = await discoverNearbyServers();
			if (servers.length > 0) {
				const loc = getLocation();
				const result = await fetchFromServers(servers,
					loc.lat && loc.lon ? { lat: loc.lat, lon: loc.lon, radiusKm: 50 } : undefined);
				posts = result.posts;
			} else {
				posts = [];
			}
		} catch (e) {
			posts = [];
		}
		loading = false;
	}

	function onLocationSet() {
		loadFeed();
	}

	$effect(() => {
		if ($location.lat && $location.lon && !searched) {
			loadFeed();
		}
	});

	let filteredPosts = $derived(
		filter === 'all' ? posts : posts.filter((p) => p.kind === filter)
	);

	/** To seed an empty server, post to it directly — there is no container to found first. */
	async function postFirst() {
		if (!isConnected()) {
			if (servers.length === 0) {
				goto('/connect');
				return;
			}
			await connectToServer(servers[0].url);
		}
		goto('/aid/new');
	}
</script>

<div class="container">
	{#if !hasLocation() && !$location.lat}
		<section class="hero">
			<h1>komun</h1>
			<div class="location-prompt">
				<LocationBar {onLocationSet} />
			</div>
			<p class="manual-link">
				<a href="/connect">Or connect to a specific server</a>
			</p>
		</section>

		<div class="hero-divider">
			<span class="divider-diamond">&#9670;</span>
		</div>
	{:else}
		<div class="feed-header">
			<LocationBar {onLocationSet} />
		</div>

		<div class="filters">
			<button class:active={filter === 'all'} onclick={() => filter = 'all'}>All</button>
			<button class:active={filter === 'need'} onclick={() => filter = 'need'}>Needs</button>
			<button class:active={filter === 'offer'} onclick={() => filter = 'offer'}>Offers</button>
			<button class:active={filter === 'resource'} onclick={() => filter = 'resource'}>Resources</button>
		</div>

		{#if loading}
			<p class="status">Searching for listings nearby...</p>
		{:else if servers.length === 0}
			<div class="empty">
				<p>No servers found nearby. Be the first to add local listings.</p>
				<p class="sub"><a href="/connect">Browse available servers</a> to join one.</p>
			</div>
		{:else if filteredPosts.length === 0}
			<div class="empty">
				<p>Found {servers.length} server{servers.length > 1 ? 's' : ''} nearby, but no posts yet.</p>
				<button class="start-btn" onclick={postFirst}>Post the first one</button>
			</div>
		{:else}
			<ul class="feed">
				{#each filteredPosts as post}
					<li><AidCard {post} /></li>
				{/each}
			</ul>
			<p class="feed-footer">
				Showing listings from {servers.length} nearby server{servers.length > 1 ? 's' : ''}
				&middot; <button class="link-btn" onclick={postFirst}>Post something</button>
			</p>
		{/if}
	{/if}
</div>

<style>
	.hero {
		text-align: center;
		padding: var(--space-10) 0 var(--space-6);
		max-width: 440px;
		margin: 0 auto;
	}

	h1 {
		font-size: var(--text-4xl);
		font-weight: 800;
		letter-spacing: -1.5px;
		font-family: var(--font-display);
		margin-bottom: 0.25rem;
	}

	.hero-divider {
		text-align: center;
		padding-bottom: var(--space-6);
	}

	.divider-diamond {
		color: var(--accent);
		font-size: 0.6rem;
		opacity: 0.4;
	}

	.location-prompt {
		margin-bottom: 1.5rem;
	}

	.manual-link {
		font-size: 0.85rem;
		color: var(--text-muted);
	}

	.feed-header {
		margin-bottom: 1rem;
	}

	.filters {
		display: flex;
		gap: 0.5rem;
		margin-bottom: 1.5rem;
	}

	.filters button {
		background: var(--bg-surface);
		color: var(--text-muted);
		padding: 0.4rem 0.8rem;
		border-radius: var(--radius);
		font-size: 0.85rem;
		border: 1px solid var(--border);
	}

	.filters button.active {
		background: var(--accent-soft);
		color: var(--accent);
		border-color: var(--accent);
	}

	.feed {
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.feed :global(li:nth-child(odd) .aid-card) {
		transform: rotate(-0.4deg);
	}

	.feed :global(li:nth-child(even) .aid-card) {
		transform: rotate(0.3deg);
	}

	.feed :global(.aid-card:hover) {
		transform: translateY(-3px) rotate(0deg) !important;
	}

	.feed-footer {
		text-align: center;
		color: var(--text-muted);
		font-size: 0.8rem;
		margin-top: 1.5rem;
		padding-top: 1rem;
		border-top: 1px solid var(--border);
	}

	.link-btn {
		background: none;
		color: var(--accent);
		font-size: inherit;
		font-weight: 600;
		text-decoration: underline;
		text-underline-offset: 2px;
		padding: 0;
		min-height: unset;
		min-width: unset;
	}

	.link-btn:hover {
		opacity: 0.8;
		text-decoration: underline;
	}

	.status {
		text-align: center;
		color: var(--text-muted);
		padding: 3rem 0;
	}

	.empty {
		text-align: center;
		padding: 3rem 0;
	}

	.empty p { color: var(--text-muted); }
	.empty .sub { font-size: 0.85rem; margin-top: 0.5rem; }

	.filters button {
		background: var(--bg-surface);
		color: var(--text-muted);
		padding: 0.4rem 0.8rem;
		border-radius: var(--radius-full);
		font-size: var(--text-xs);
		border: 1px solid var(--border);
	}

	.filters button.active {
		background: var(--accent-soft);
		color: var(--accent);
		border-color: var(--accent);
	}

	.start-btn {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.75rem 1.5rem;
		border-radius: var(--radius-full);
		font-weight: 700;
		font-size: var(--text-base);
		margin-top: 1rem;
		transition: transform var(--transition-fast);
	}

	.start-btn:hover {
		transform: translateY(-1px);
	}
</style>
