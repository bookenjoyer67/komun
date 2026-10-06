<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		description?: string;
		/** Optional call to action, e.g. a .btn-primary link. */
		children?: Snippet;
	}

	let { title, description, children }: Props = $props();
</script>

<div class="empty-state">
	<p class="title">{title}</p>
	{#if description}
		<p class="description">{description}</p>
	{/if}
	{#if children}
		<div class="action">{@render children()}</div>
	{/if}
</div>

<style>
	.empty-state {
		text-align: center;
		padding: var(--space-8) 0;
		color: var(--text-muted);
	}

	.title {
		font-size: var(--text-base);
	}

	.description {
		font-size: var(--text-sm);
		margin-top: var(--space-2);
	}

	.action {
		margin-top: var(--space-4);
	}
</style>
