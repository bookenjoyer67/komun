<script lang="ts">
	import { onMount } from 'svelte';
	import LocationMap from '$lib/components/LocationMap.svelte';

	// Typed locally because this route does not own the shared API client module.
	interface PostLocation {
		id: string;
		title: string;
		location_name: string | null;
		location_lat: number | null;
		location_lon: number | null;
	}

	let posts = $state<PostLocation[]>([]);
	let loading = $state(true);
	let notMerged = $state(false);
	let error = $state<string | null>(null);

	onMount(loadPosts);

	async function loadPosts() {
		loading = true;
		notMerged = false;
		error = null;
		try {
			const res = await fetch('/api/posts');
			if (res.status === 404) {
				// A 404 means the endpoint is not available here, not a bug.
				notMerged = true;
				posts = [];
				return;
			}
			if (!res.ok) throw new Error(`Request failed (${res.status})`);
			const data = await res.json();
			const list = Array.isArray(data) ? data : Array.isArray(data?.posts) ? data.posts : [];
			posts = list;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load posts';
			posts = [];
		} finally {
			loading = false;
		}
	}

	const located = $derived(
		posts.filter((post) => post.location_lat != null && post.location_lon != null)
	);

	const markers = $derived(
		located.map((post) => ({
			lat: post.location_lat as number,
			lon: post.location_lon as number,
			label: post.location_name ?? post.title
		}))
	);

	const center = $derived(
		markers.length > 0
			? { lat: markers[0].lat, lon: markers[0].lon, zoom: 12 }
			: { lat: 20, lon: 0, zoom: 2 }
	);
</script>

<div class="map-page">
	<header class="map-header">
		<h1>Map</h1>
		<p class="tagline">Public posts that carry a location.</p>
	</header>

	{#if notMerged}
		<p class="note">
			The flat posts endpoint is not available on this node yet, so the map is empty.
		</p>
	{/if}
	{#if error}
		<p class="note error">{error}</p>
	{/if}

	<div class="map-layout">
		<div class="map-panel">
			<LocationMap lat={center.lat} lon={center.lon} zoom={center.zoom} {markers} />
		</div>

		<aside class="post-list">
			<h2>Located posts</h2>
			{#if loading}
				<p class="note">Loading posts…</p>
			{:else if located.length === 0}
				<p class="note">No located posts yet.</p>
			{:else}
				<ul>
					{#each located as post (post.id)}
						<li>
							<span class="post-title">{post.title}</span>
							{#if post.location_name}
								<span class="where">{post.location_name}</span>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</aside>
	</div>
</div>

<style>
	.map-page {
		max-width: 1100px;
		margin: 0 auto;
		padding: var(--space-6, 1.5rem) var(--space-4, 1rem);
	}

	.map-header h1 {
		font-size: var(--text-2xl, 1.5rem);
		margin: 0 0 0.25rem;
	}

	.tagline {
		color: var(--text-muted);
		font-size: var(--text-sm, 0.875rem);
		margin: 0;
	}

	.note {
		color: var(--text-muted);
		font-size: var(--text-sm, 0.875rem);
		margin: 0.75rem 0;
	}

	.note.error {
		color: var(--critical, #c0392b);
	}

	.map-layout {
		display: grid;
		grid-template-columns: minmax(0, 2fr) minmax(220px, 1fr);
		gap: 1rem;
		margin-top: 1rem;
	}

	.map-panel {
		min-height: 320px;
	}

	.post-list {
		border: 1px solid var(--border);
		border-radius: var(--radius, 8px);
		padding: 0.75rem 1rem;
		max-height: 480px;
		overflow-y: auto;
	}

	.post-list h2 {
		font-size: var(--text-base, 1rem);
		margin: 0 0 0.5rem;
	}

	.post-list ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.post-list li {
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.post-title {
		font-size: var(--text-sm, 0.875rem);
	}

	.where {
		font-size: var(--text-xs, 0.75rem);
		color: var(--text-muted);
	}

	@media (max-width: 720px) {
		.map-layout {
			grid-template-columns: 1fr;
		}
	}
</style>
