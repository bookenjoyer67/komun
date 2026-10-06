<script lang="ts">
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import { getActiveServer } from '$lib/stores/server';
	import { auth } from '$lib/stores/auth';
	import LinkPreview from '$lib/components/LinkPreview.svelte';
	import RespondModal from '$lib/components/RespondModal.svelte';
	import { formatPrice } from '$lib/api/market';
	import type { PostLike } from '$lib/api/types';

	/** The marketplace facet a listing/want carries; `PostLike` deliberately omits it. */
	interface PostDetail extends PostLike {
		market_listed?: boolean;
		price_cents?: number | null;
		currency?: string | null;
		price_negotiable?: boolean;
	}

	/** Posts are flat and server-wide, so the id alone addresses one. */
	let post = $state<PostDetail | null>(null);
	let error = $state('');
	let loading = $state(true);
	let showModal = $state(false);

	const kindLabels: Record<string, string> = {
		resource: 'Resource', need: 'Need', offer: 'Offer', listing: 'Listing', want: 'Want'
	};

	const urls: string[] = $derived(post?.body
		? [...post.body.matchAll(/https?:\/\/[^\s<>"]+/g)].map((m) => m[0].replace(/[.,;:!?)]+$/, ''))
		: []);

	let myUserId = $derived((() => {
		const server = getActiveServer();
		if (!server) return null;
		return $auth.servers?.[server]?.userId || null;
	})());

	onMount(async () => {
		try {
			post = await api.posts.get($page.params.id as string);
		} catch (e: any) {
			error = e.message || 'Post not found';
		}
		loading = false;
	});

	function timeAgo(dateStr: string): string {
		const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
		return `${Math.floor(seconds / 86400)}d ago`;
	}
</script>

<svelte:head>
	<title>{post?.title || 'Post'} — Komun</title>
	<meta name="description" content={post?.body?.slice(0, 200) || 'A post on Komun'} />
	<meta property="og:title" content={post?.title || 'Post'} />
	<meta property="og:description" content={post?.body?.slice(0, 200) || 'A post on Komun'} />
	<meta property="og:type" content="article" />
	<meta name="twitter:card" content="summary" />
</svelte:head>

<div class="container">
	{#if loading}
		<p class="status">Loading...</p>
	{:else if error}
		<p class="status error">{error}</p>
	{:else if post}
		<a href="/aid" class="back">&larr; All listings</a>

		<article class="post-detail">
			<div class="meta">
				<span class="kind kind-{post.kind}">{kindLabels[post.kind] || post.kind}</span>
				<span class="category">{post.category}</span>
				{#if post.urgency}
					<span class="urgency">{post.urgency}</span>
				{/if}
				<span class="time">{timeAgo(post.created_at)}</span>
			</div>

			<h1>{post.title}</h1>

			{#if post.market_listed || post.kind === 'listing' || post.kind === 'want'}
				<p class="market-price">{formatPrice(post.price_cents, post.currency, post.price_negotiable)}</p>
			{/if}

			{#if post.body}
				<p class="body">{post.body}</p>
			{/if}

			{#each urls as url}
				<LinkPreview {url} />
			{/each}

			{#if post.location_name}
				<p class="location">📍 {post.location_name}</p>
			{/if}

			{#if post.contact_method}
				<p class="contact">📞 {post.contact_method}</p>
			{/if}

			{#if post.images?.length}
				<div class="images">
					{#each post.images as img}
						<img src={'/post-images/' + img} alt="" />
					{/each}
				</div>
			{/if}

			{#if post.status === 'active' && post.author_id && post.author_id !== myUserId}
				<button class="btn-primary respond-btn" onclick={() => showModal = true}>
					{#if post.kind === 'need'}I can help{:else if post.kind === 'offer'}Request this{:else if post.kind === 'listing' || post.kind === 'want'}Make an offer{:else}Respond{/if}
				</button>
			{/if}
		</article>
	{/if}
</div>

{#if showModal && post}
	<RespondModal
		post={{ id: post.id, title: post.title, kind: post.kind, server_url: post.server_url || getActiveServer() || '', author_id: post.author_id }}
		onClose={() => showModal = false}
	/>
{/if}

<style>
	.container { max-width: 640px; margin: 0 auto; padding: 2rem 1rem; }
	.back { color: var(--text-muted); font-size: 0.85rem; display: inline-block; margin-bottom: 1.5rem; }
	.post-detail { background: var(--bg-surface); border: 1px solid var(--border); border-radius: var(--radius-md); padding: var(--space-5); }
	.meta { display: flex; gap: 0.5rem; align-items: center; margin-bottom: 0.75rem; font-size: var(--text-xs); }
	.kind { padding: 0.15rem 0.5rem; border-radius: var(--radius-full); font-weight: 700; text-transform: uppercase; font-size: 0.65rem; letter-spacing: 0.3px; }
	.kind-need { background: var(--kind-need-soft); color: var(--kind-need, var(--critical)); }
	.kind-offer { background: var(--kind-offer-soft); color: var(--kind-offer, var(--success)); }
	.kind-resource { background: var(--kind-resource-soft); color: var(--kind-resource); }
	.category { color: var(--text-muted); text-transform: capitalize; }
	.urgency { font-weight: 700; text-transform: uppercase; color: var(--text-muted); }
	.time { color: var(--text-muted); margin-left: auto; }
	h1 { font-size: var(--text-2xl); margin-bottom: 0.75rem; }
	.body { color: var(--text); font-size: var(--text-base); line-height: 1.7; margin-bottom: var(--space-4); white-space: pre-wrap; }
	.market-price { color: var(--accent); font-size: var(--text-xl); font-weight: 700; margin-bottom: var(--space-4); }
	.location { color: var(--text-muted); font-size: var(--text-sm); margin-bottom: var(--space-2); }
	.contact { color: var(--text-muted); font-size: var(--text-sm); margin-bottom: var(--space-2); }
	.images { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-top: var(--space-3); }
	.images img { max-width: 100%; max-height: 400px; border-radius: var(--radius-md); border: 1px solid var(--border); }
	.respond-btn { margin-top: var(--space-4); background: var(--accent); color: var(--text-on-accent); padding: var(--space-2) var(--space-4); border-radius: var(--radius-full); font-weight: 600; }
	.status { text-align: center; color: var(--text-muted); padding: 3rem 0; }
	.error { color: var(--critical); }
</style>
