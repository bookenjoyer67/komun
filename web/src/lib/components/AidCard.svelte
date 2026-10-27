<script lang="ts">
	import RespondModal from './RespondModal.svelte';
	import LinkPreview from './LinkPreview.svelte';
	import LocationMap from './LocationMap.svelte';
	import { auth, getToken } from '$lib/stores/auth';
	import { getActiveServer } from '$lib/stores/server';
	import { coarsenCoordinate } from '$lib/geo';
	import { closedLabel } from '$lib/api/market';
	import { categoryLabel } from '$lib/api/categories';
	import type { PostLike } from '$lib/api/types';

	interface Props {
		post: PostLike;
		onFulfill?: (id: string) => void;
		onEdit?: (post: PostLike) => void;
		onDelete?: (id: string) => void;
		editing?: boolean;
		editTitle?: string;
		editBody?: string;
		onEditTitleChange?: (v: string) => void;
		onEditBodyChange?: (v: string) => void;
		onSaveEdit?: () => void;
		onCancelEdit?: () => void;
		editExistingImages?: string[];
		editImageFiles?: File[];
		editImagePreviews?: string[];
		onEditImages?: (e: Event) => void;
		onRemoveEditImage?: (i: number) => void;
	}

	let { post,
		onFulfill, onEdit, onDelete,
		editing = false, editTitle = '', editBody = '',
		onEditTitleChange, onEditBodyChange,
		onSaveEdit, onCancelEdit,
		editExistingImages = [], editImageFiles = [], editImagePreviews = [],
		onEditImages, onRemoveEditImage
	}: Props = $props();
	let showModal = $state(false);
	let showMap = $state(false);

	/** A pause is the stored `matched` status; readers see PAUSED_LABEL, never the stored word. */
	const PAUSED_STATUS = 'matched';
	const PAUSED_LABEL = 'Paused — not taking responses';

	// Coarsened here too: a post from another server may still carry exact coordinates.
	const mapPoint = $derived(post.location_lat != null && post.location_lon != null
		? { lat: coarsenCoordinate(post.location_lat), lon: coarsenCoordinate(post.location_lon) }
		: null);

	let myUserId = $derived((() => {
		const server = getActiveServer();
		if (!server) return null;
		return $auth.servers?.[server]?.userId || null;
	})());

	const kindLabels: Record<string, string> = {
		resource: 'Resource', need: 'Need', offer: 'Offer', listing: 'Listing', want: 'Want'
	};

	function timeAgo(dateStr: string): string {
		const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
		return `${Math.floor(seconds / 86400)}d ago`;
	}

	function handleEditFileInput() {
		const el = document.querySelector('.edit-file-input-' + post.id) as HTMLInputElement;
		el?.click();
	}

	const urls: string[] = $derived(post.body
		? [...post.body.matchAll(/https?:\/\/[^\s<>"]+/g)].map(m => m[0].replace(/[.,;:!?)]+$/, ''))
		: []);
</script>

<article class="aid-card" class:editing>
	{#if editing}
		<form class="edit-form" onsubmit={(e) => { e.preventDefault(); onSaveEdit?.(); }}>
			<input type="text" value={editTitle} oninput={(e) => onEditTitleChange?.((e.target as HTMLInputElement).value)} placeholder="Title" />
			<textarea value={editBody} oninput={(e) => onEditBodyChange?.((e.target as HTMLTextAreaElement).value)} placeholder="Details" rows="2"></textarea>
			{#if editExistingImages.length > 0}
				<div class="edit-images">
					{#each editExistingImages as img}
						<img src={'/post-images/' + img} alt="" class="post-thumb" />
					{/each}
				</div>
			{/if}
			<div class="edit-image-previews">
				{#each editImagePreviews as preview, i}
					<div class="preview-item">
						<img src={preview} alt="" />
						<button type="button" class="remove-img" onclick={() => onRemoveEditImage?.(i)}>&times;</button>
					</div>
				{/each}
			</div>
			{#if (editExistingImages.length + editImageFiles.length) < 5}
				<input type="file" accept="image/png,image/jpeg,image/webp" multiple onchange={onEditImages} class="edit-file-input edit-file-input-{post.id}" />
				<button type="button" class="btn-ghost add-img-btn" onclick={handleEditFileInput}>Add images</button>
			{/if}
			<div class="edit-actions">
				<button type="submit" class="save-btn">Save</button>
				<button type="button" class="cancel-btn" onclick={() => onCancelEdit?.()}>Cancel</button>
			</div>
		</form>
	{:else}
		<div class="card-top">
			<span class="kind kind-{post.kind}">{kindLabels[post.kind]}</span>
			<span class="category">{categoryLabel(post)}</span>
			{#if post.urgency}
				<span class="urgency" data-level={post.urgency}>{post.urgency}</span>
			{/if}
			<span class="time">{timeAgo(post.created_at)}</span>
		</div>

		<h3>{post.title}</h3>

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
			<div class="post-images">
				{#each post.images.slice(0, 3) as img}
					<img src={'/post-images/' + img} alt="" class="post-thumb" />
				{/each}
				{#if post.images.length > 3}
					<span class="more-images">+{post.images.length - 3}</span>
				{/if}
			</div>
		{/if}

		<div class="footer">
			<span class="origin">{post.server_name || ''}</span>
			<div class="footer-actions">
				{#if mapPoint}
					<button class="map-btn" onclick={() => showMap = true} title="View on map">📍</button>
				{/if}
				<a class="map-btn permalink" href="/p/{post.id}" title="Open this post">🔗</a>
				{#if post.status === PAUSED_STATUS}
					<span class="paused-note">{PAUSED_LABEL}</span>
				{:else if post.status === 'fulfilled'}
					<span class="fulfilled-badge">{closedLabel(post)}</span>
				{/if}
				{#if post.author_id === myUserId && (onFulfill || onEdit || onDelete)}
					{#if post.status !== 'fulfilled'}
						{#if onFulfill}<button class="action-btn fulfill" onclick={() => onFulfill(post.id)}>Fulfill</button>{/if}
						{#if onEdit}<button class="action-btn edit" onclick={() => onEdit(post)}>Edit</button>{/if}
						{#if onDelete}<button class="action-btn delete" onclick={() => onDelete(post.id)}>Delete</button>{/if}
					{/if}
				{:else if post.author_id !== myUserId}
					{#if post.status === 'active'}
						<button class="btn-primary respond-btn" onclick={() => showModal = true}>
							{#if post.kind === 'need'}I can help{:else if post.kind === 'offer'}Request this{:else}Respond{/if}
						</button>
					{/if}
				{:else}
					<span class="your-post">Your post</span>
				{/if}
			</div>
		</div>
	{/if}
</article>

{#if showMap && mapPoint}
	<!--
		The backdrop dismisses on click and must do the same from the keyboard; it is focusable so
		`role="dialog"` means something to a screen reader.
	-->
	<div
		class="map-overlay"
		role="dialog"
		aria-modal="true"
		aria-label="Post location"
		tabindex="-1"
		onclick={() => showMap = false}
		onkeydown={(e) => { if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') showMap = false; }}
	>
		<!-- Without this, a click or drag on the map would reach the backdrop and close it. -->
		<div
			class="map-popout"
			role="presentation"
			onclick={(e) => e.stopPropagation()}
			onkeydown={(e) => e.stopPropagation()}
		>
			<button class="map-close" onclick={() => showMap = false}>&times;</button>
			<div class="map-frame">
				<LocationMap lat={mapPoint.lat} lon={mapPoint.lon} markers={[mapPoint]} />
			</div>
		</div>
	</div>
{/if}

{#if showModal}
	<RespondModal
		post={{ id: post.id, title: post.title, kind: post.kind, server_url: post.server_url || getActiveServer() || '', author_id: post.author_id }}
		onClose={() => showModal = false}
	/>
{/if}

<style>
	.aid-card {
		background: var(--bg-surface);
		border: 1px solid transparent;
		border-radius: var(--radius-md);
		padding: var(--space-4);
		box-shadow: 2px 3px 0 rgba(0,0,0,0.15), 4px 6px 12px rgba(0,0,0,0.2);
		transition: transform var(--transition-base), box-shadow var(--transition-base), border-color var(--transition-fast);
	}

	.aid-card:hover {
		transform: translateY(-3px) rotate(0deg);
		box-shadow: 3px 5px 0 rgba(0,0,0,0.2), 6px 10px 20px rgba(0,0,0,0.3);
		border-color: var(--accent);
	}

	.card-top { display: flex; gap: 0.5rem; align-items: center; margin-bottom: 0.5rem; font-size: var(--text-xs); }
	.kind { padding: 0.15rem 0.5rem; border-radius: var(--radius-full); font-weight: 700; text-transform: uppercase; font-size: 0.65rem; letter-spacing: 0.3px; }
	.kind-need { background: var(--kind-need-soft); color: var(--kind-need, var(--critical)); }
	.kind-offer { background: var(--kind-offer-soft); color: var(--kind-offer, var(--success)); }
	.kind-resource { background: var(--kind-resource-soft); color: var(--kind-resource); }
	.category { color: var(--text-muted); text-transform: capitalize; font-size: 0.7rem; }
	.urgency { font-weight: 700; text-transform: uppercase; font-size: 0.65rem; }
	.urgency[data-level="critical"] { color: var(--critical); }
	.urgency[data-level="high"] { color: var(--warning); }
	.urgency[data-level="medium"], .urgency[data-level="low"] { color: var(--text-muted); }
	.time { color: var(--text-muted); margin-left: auto; }
	h3 { font-size: var(--text-lg); margin-bottom: 0.3rem; font-weight: 700; }
	.body { color: var(--text-muted); font-size: var(--text-sm); margin-bottom: var(--space-3); line-height: 1.5; }
	.location { color: var(--text-muted); font-size: var(--text-xs); margin-bottom: var(--space-1); }
	.contact { color: var(--text-muted); font-size: var(--text-xs); margin-bottom: var(--space-2); }

	.footer { display: flex; justify-content: space-between; align-items: center; margin-top: var(--space-3); padding-top: var(--space-3); border-top: 1px solid var(--border); }
	.footer-actions { display: flex; align-items: center; gap: 0.4rem; }

	.map-btn { background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 0.25rem 0.5rem; font-size: 0.9rem; min-height: unset; min-width: unset; cursor: pointer; transition: border-color var(--transition-fast); }
	.map-btn:hover { border-color: var(--accent); }
	.map-overlay { position: fixed; inset: 0; background: var(--overlay); display: flex; align-items: center; justify-content: center; z-index: 1000; padding: 1rem; }
	.map-popout { position: relative; width: 100%; max-width: 500px; aspect-ratio: 1; background: var(--bg-surface); border: 1px solid var(--border); border-radius: var(--radius-lg); overflow: hidden; }
	/* Leaflet's panes and controls sit at z-index 400 and up; isolating them keeps the close button above the map. */
	.map-frame { position: absolute; inset: 0; isolation: isolate; }
	.map-close { position: absolute; top: 0.5rem; right: 0.5rem; z-index: 1; background: var(--bg-surface); color: var(--text); border: 1px solid var(--border); border-radius: 50%; width: 28px; height: 28px; font-size: 1rem; display: flex; align-items: center; justify-content: center; padding: 0; min-height: unset; min-width: unset; }

	.origin { font-size: var(--text-sm); color: var(--text); font-weight: 600; }
	.permalink { display: inline-flex; align-items: center; text-decoration: none; line-height: 1; }
	.respond-btn { font-size: var(--text-xs); padding: var(--space-1) var(--space-3); }
	.your-post { color: var(--text-muted); font-size: var(--text-xs); font-style: italic; }
	.paused-note { color: var(--text-muted); font-size: var(--text-xs); font-weight: 600; }
	.fulfilled-badge { font-size: var(--text-xs); color: var(--success); font-weight: 600; }

	.action-btn { padding: 0.3rem 0.6rem; border-radius: var(--radius-full); font-size: 0.75rem; font-weight: 600; border: 1px solid; cursor: pointer; }
	.action-btn.fulfill { background: var(--success-softer); color: var(--success); border-color: var(--success); }
	.action-btn.edit { background: var(--bg-elevated); color: var(--text-muted); border-color: var(--border); }
	.action-btn.delete { background: var(--critical-softer); color: var(--critical); border-color: var(--critical); }

	.post-images { display: flex; gap: 0.4rem; margin: 0.5rem 0; align-items: center; }
	.post-thumb { width: 72px; height: 72px; object-fit: cover; border-radius: var(--radius-md); border: 1px solid var(--border); }
	.more-images { font-size: 0.75rem; color: var(--text-muted); background: var(--bg-elevated); padding: 0.2rem 0.5rem; border-radius: var(--radius-sm); }

	.edit-form { display: flex; flex-direction: column; gap: 0.5rem; }
	.edit-form input, .edit-form textarea { background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); padding: 0.5rem; color: var(--text); font-size: 0.9rem; width: 100%; box-sizing: border-box; }
	.edit-images, .edit-image-previews { display: flex; flex-wrap: wrap; gap: 0.4rem; }
	.preview-item { position: relative; width: 64px; height: 64px; }
	.preview-item img { width: 100%; height: 100%; object-fit: cover; border-radius: var(--radius-sm); border: 1px solid var(--border); }
	.remove-img { position: absolute; top: -6px; right: -6px; background: var(--critical); color: var(--text-on-critical); border-radius: 50%; width: 20px; height: 20px; font-size: 12px; line-height: 1; display: flex; align-items: center; justify-content: center; padding: 0; min-height: unset; min-width: unset; cursor: pointer; }
	.edit-file-input { display: none; }
	.add-img-btn { font-size: var(--text-xs); }
	.edit-actions { display: flex; gap: 0.4rem; }
	.save-btn { background: var(--accent); color: var(--text-on-accent); padding: 0.3rem 0.8rem; border-radius: var(--radius); font-size: 0.8rem; font-weight: 600; cursor: pointer; }
	.cancel-btn { background: var(--bg-elevated); color: var(--text-muted); padding: 0.3rem 0.8rem; border-radius: var(--radius); font-size: 0.8rem; border: 1px solid var(--border); cursor: pointer; }

	@media (max-width: 480px) {
		.footer { flex-direction: column; align-items: flex-start; gap: 0.5rem; }
	}
</style>
