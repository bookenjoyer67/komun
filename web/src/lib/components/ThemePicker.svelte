<script lang="ts">
	import { onMount } from 'svelte';
	import { themeName, setTheme, getAll } from '$lib/stores/theme';

	interface Props {
		shown: boolean;
		onClose: () => void;
	}

	let { shown, onClose }: Props = $props();

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onClose();
	}

	$effect(() => {
		if (shown) {
			window.addEventListener('keydown', handleKeydown);
			return () => window.removeEventListener('keydown', handleKeydown);
		}
	});

	function select(name: string) {
		setTheme(name);
		onClose();
	}
</script>

{#if shown}
<div class="overlay" role="dialog" aria-modal="true" tabindex="-1">
	<div class="modal">
		<div class="modal-header">
			<h2>Theme</h2>
			<button class="close-btn" onclick={onClose} aria-label="Close">&times;</button>
		</div>
		<div class="theme-grid">
			{#each getAll() as t}
				<button
					class="theme-card"
					class:active={$themeName === t.name}
					onclick={() => select(t.name)}
					title={t.label}
				>
					<div class="swatch-strip">
						<span class="swatch-seg" style="background: {t.colors['bg-surface']}"></span>
						<span class="swatch-seg" style="background: {t.colors.accent}"></span>
						<span class="swatch-seg" style="background: {t.colors.text}"></span>
					</div>
					<span class="theme-name">{t.label}</span>
				</button>
			{/each}
		</div>
	</div>
</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: var(--overlay);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 1000;
		padding: 1rem;
	}

	.modal {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 1.5rem;
		max-width: 560px;
		width: 100%;
		max-height: 80vh;
		display: flex;
		flex-direction: column;
		position: relative;
	}

	.modal-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 1rem;
	}

	.modal-header h2 {
		font-size: 1.2rem;
		margin: 0;
	}

	.close-btn {
		background: none;
		color: var(--text-muted);
		font-size: 1.5rem;
		line-height: 1;
		padding: 0 0.25rem;
	}

	.close-btn:hover { color: var(--text); }

	.theme-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.5rem;
		overflow-y: auto;
		padding-right: 0.25rem;
	}

	@media (max-width: 480px) {
		.theme-grid {
			grid-template-columns: repeat(2, 1fr);
		}
	}

	.theme-card {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.35rem;
		background: var(--bg);
		border: 2px solid var(--border);
		border-radius: var(--radius);
		padding: 0.6rem;
		cursor: pointer;
		transition: border-color var(--transition-fast);
		min-height: unset;
	}

	.theme-card:hover { border-color: var(--text-muted); }

	.theme-card.active {
		border-color: var(--accent);
	}

	.theme-card.active:hover {
		border-color: var(--accent);
	}

	.swatch-strip {
		display: flex;
		flex-direction: column;
		width: 100%;
		height: 32px;
		border-radius: var(--radius-sm);
		overflow: hidden;
	}

	.swatch-seg { flex: 1; }

	.theme-name {
		font-size: 0.7rem;
		color: var(--text-muted);
		text-align: center;
		line-height: 1.2;
	}

	.theme-card.active .theme-name {
		color: var(--text);
	}
</style>
