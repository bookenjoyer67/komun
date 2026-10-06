<script lang="ts">
	import {
		conditionLabel,
		formatPrice,
		MARKET_KIND_LABELS,
		type MarketPost
	} from '$lib/api/market';

	interface Props {
		post: MarketPost;
	}

	let { post }: Props = $props();

	let kindLabel = $derived(MARKET_KIND_LABELS[post.kind] ?? post.kind);
	let category = $derived(post.category_label || post.category);
	let price = $derived(formatPrice(post.price_cents, post.currency, post.price_negotiable));
	let condition = $derived(conditionLabel(post.item_condition));
</script>

<article class="market-card" data-kind={post.kind} data-empty-price={post.price_cents == null}>
	<div class="card-top">
		<span class="kind kind-{post.kind}">{kindLabel}</span>
		{#if category}
			<span class="category">{category}</span>
		{/if}
		<a class="permalink" href="/p/{post.id}" title="Open this post" aria-label="Open this post">🔗</a>
	</div>

	<h3><a href="/p/{post.id}">{post.title}</a></h3>

	<p class="price">{price}</p>

	<dl class="facts">
		{#if condition}
			<div class="fact"><dt>Condition</dt><dd>{condition}</dd></div>
		{/if}
		{#if post.location_name}
			<div class="fact"><dt>Location</dt><dd>{post.location_name}</dd></div>
		{/if}
	</dl>
</article>

<style>
	.market-card {
		background: var(--bg-surface);
		border: 1px solid transparent;
		border-radius: var(--radius-md);
		padding: var(--space-4);
		box-shadow: 2px 3px 0 rgba(0, 0, 0, 0.15), 4px 6px 12px rgba(0, 0, 0, 0.2);
		transition: transform var(--transition-base), box-shadow var(--transition-base),
			border-color var(--transition-fast);
	}

	.market-card:hover {
		transform: translateY(-3px);
		border-color: var(--accent);
	}

	.card-top {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		margin-bottom: 0.5rem;
		font-size: var(--text-xs);
	}

	.kind {
		padding: 0.15rem 0.5rem;
		border-radius: var(--radius-full);
		font-weight: 700;
		text-transform: uppercase;
		font-size: 0.65rem;
		letter-spacing: 0.3px;
	}

	.kind-listing {
		background: var(--kind-offer-soft);
		color: var(--kind-offer, var(--success));
	}

	.kind-want {
		background: var(--kind-need-soft);
		color: var(--kind-need, var(--critical));
	}

	.category {
		color: var(--text-muted);
		text-transform: capitalize;
		font-size: 0.7rem;
	}

	.permalink {
		margin-left: auto;
		text-decoration: none;
		line-height: 1;
	}

	h3 {
		font-size: var(--text-lg);
		margin-bottom: 0.3rem;
		font-weight: 700;
	}

	h3 a {
		color: var(--text);
		text-decoration: none;
	}

	h3 a:hover {
		color: var(--accent);
	}

	.price {
		color: var(--accent);
		font-size: var(--text-base);
		font-weight: 700;
		margin-bottom: var(--space-2);
	}

	.market-card[data-empty-price='true'] .price {
		color: var(--text-muted);
		font-weight: 600;
	}

	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem 1rem;
		margin: 0;
	}

	.fact {
		display: flex;
		gap: 0.35rem;
		font-size: var(--text-xs);
	}

	.fact dt {
		color: var(--text-muted);
	}

	.fact dt::after {
		content: ':';
	}

	.fact dd {
		margin: 0;
		color: var(--text);
	}
</style>
