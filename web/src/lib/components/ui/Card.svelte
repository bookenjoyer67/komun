<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		children: Snippet;
		/** Set false for a card that manages its own internal padding. */
		padded?: boolean;
	}

	let { children, padded = true }: Props = $props();
</script>

<div class="card" class:padded>
	{@render children()}
</div>

<style>
	.card {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-sm);
	}

	.card.padded {
		padding: var(--space-4);
	}
</style>
