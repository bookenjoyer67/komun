<script lang="ts">
	import { goto } from '$app/navigation';
	import { requireAuth } from '$lib/stores/auth';
	import { api } from '$lib/api/client';
	import LocationMap from '$lib/components/LocationMap.svelte';

	let kind = $state('need');
	let category = $state('other');
	let title = $state('');
	let body = $state('');
	let urgency = $state('medium');
	let expiresIn = $state('7');
	let locationName = $state('');
	let locationLat: number | null = $state(null);
	let locationLon: number | null = $state(null);
	let contactMethod = $state('');
	let error = $state('');
	let loading = $state(false);
	let imageFiles: File[] = $state([]);
	let imagePreviews: string[] = $state([]);

	const expiryDefaults: Record<string, string> = { need: '7', offer: '14', resource: '0' };

	function setKind(k: string) {
		kind = k;
		expiresIn = expiryDefaults[k] || '7';
	}

	function getExpiresAt(): string | null {
		const days = parseInt(expiresIn);
		if (!days) return null;
		return new Date(Date.now() + days * 86400000).toISOString();
	}

	/* Coordinates stay optional, and the map stays Leaflet-only — no iframe, relay or map credentials. */
	const mapCenter = { lat: 20, lon: 0, zoom: 2 };

	function setPickedLocation(coords: { lat: number; lon: number }) {
		locationLat = coords.lat;
		locationLon = coords.lon;
	}

	function clearLocation() {
		locationLat = null;
		locationLon = null;
	}

	function handleImages(e: Event) {
		const files = (e.target as HTMLInputElement).files;
		if (!files) return;
		const newFiles: File[] = [];
		const newPreviews: string[] = [];
		for (const file of files) {
			if (imageFiles.length + newFiles.length >= 5) break;
			if (!file.type.match(/image\/(png|jpeg|webp)/)) continue;
			newFiles.push(file);
			newPreviews.push(URL.createObjectURL(file));
		}
		imageFiles = [...imageFiles, ...newFiles];
		imagePreviews = [...imagePreviews, ...newPreviews];
	}

	function removeImage(i: number) {
		URL.revokeObjectURL(imagePreviews[i]);
		imageFiles = imageFiles.filter((_, j) => j !== i);
		imagePreviews = imagePreviews.filter((_, j) => j !== i);
	}

	function submit() {
		requireAuth(async () => {
			if (!title.trim()) { error = 'Title is required'; return; }
			loading = true;
			error = '';
			try {
				const post = await api.posts.create({
					kind,
					category,
					title: title.trim(),
					body: body.trim() || null,
					urgency: kind === 'need' ? urgency : null,
					expires_at: getExpiresAt(),
					location_name: locationName.trim() || null,
					location_lat: locationLat,
					location_lon: locationLon,
					contact_method: contactMethod.trim() || null,
				});
				if (imageFiles.length > 0) await api.posts.addImages(post.id, imageFiles);
				goto(`/p/${post.id}`);
			} catch (e: any) {
				error = e.message || 'Failed to create post';
			} finally {
				loading = false;
			}
		});
	}
</script>

<div class="container">
	<h1>New Post</h1>

	<p class="hint">
		Selling something, or looking to buy? <a href="/market/new">Post it on the market</a> — market
		listings carry a price, a currency and a condition.
	</p>

	<form onsubmit={(e) => { e.preventDefault(); submit(); }}>
		<label>
			<span>Type</span>
			<div class="kind-selector">
				<button type="button" class:active={kind === 'need'} onclick={() => setKind('need')}>Need</button>
				<button type="button" class:active={kind === 'offer'} onclick={() => setKind('offer')}>Offer</button>
				<button type="button" class:active={kind === 'resource'} onclick={() => setKind('resource')}>Resource</button>
			</div>
		</label>

		<label>
			<span>Category</span>
			<select bind:value={category}>
				<option value="food">Food</option>
				<option value="shelter">Shelter</option>
				<option value="health">Health</option>
				<option value="transport">Transport</option>
				<option value="education">Education</option>
				<option value="labor">Labor</option>
				<option value="legal">Legal</option>
				<option value="other">Other</option>
			</select>
		</label>

		<label>
			<span>Title</span>
			<input type="text" bind:value={title} placeholder="What do you need or offer?" maxlength="200" />
		</label>

		<label>
			<span>Details (optional)</span>
			<textarea bind:value={body} placeholder="More info..." rows="3"></textarea>
		</label>

		<label>
			<span>Images (optional, max 5)</span>
			<div class="image-previews">
				{#each imagePreviews as preview, i}
					<div class="preview-item">
						<img src={preview} alt="" />
						<button type="button" class="remove-img" onclick={() => removeImage(i)}>&times;</button>
					</div>
				{/each}
			</div>
			{#if imageFiles.length < 5}
				<input type="file" accept="image/png,image/jpeg,image/webp" multiple onchange={handleImages} class="file-input" />
				<button type="button" class="btn-ghost img-btn" onclick={() => (document.querySelector('.file-input') as HTMLInputElement)?.click()}>
					{imageFiles.length > 0 ? 'Add more' : 'Add images'}
				</button>
			{/if}
			<small>PNG, JPEG, or WebP. Max 5MB each.</small>
		</label>

		{#if kind === 'need'}
			<label>
				<span>Urgency</span>
				<select bind:value={urgency}>
					<option value="critical">Critical</option>
					<option value="high">High</option>
					<option value="medium">Medium</option>
					<option value="low">Low</option>
				</select>
			</label>
		{/if}

		<label>
			<span>Expires in</span>
			<select bind:value={expiresIn}>
				<option value="1">1 day</option>
				<option value="3">3 days</option>
				<option value="7">1 week</option>
				<option value="14">2 weeks</option>
				<option value="30">1 month</option>
				<option value="0">Never</option>
			</select>
		</label>

		<label>
			<span>Location (optional)</span>
			<input type="text" bind:value={locationName} placeholder="Neighborhood or area" />
		</label>

		<div class="location-picker">
			<LocationMap
				lat={mapCenter.lat}
				lon={mapCenter.lon}
				zoom={mapCenter.zoom}
				pickable
				pickedLat={locationLat}
				pickedLon={locationLon}
				onpick={setPickedLocation}
			/>
			{#if locationLat != null && locationLon != null}
				<p class="coords">
					Pinned at {locationLat.toFixed(5)}, {locationLon.toFixed(5)}
					<button type="button" class="clear-pin" onclick={clearLocation}>Remove pin</button>
				</p>
			{:else}
				<p class="hint">Click the map to drop a pin. A post can be created without one.</p>
			{/if}
		</div>

		<label>
			<span>Contact method (optional)</span>
			<input type="text" bind:value={contactMethod} placeholder="How should people reach you?" />
		</label>

		{#if error}
			<p class="error">{error}</p>
		{/if}

		<button type="submit" disabled={loading}>
			{loading ? 'Posting...' : 'Post'}
		</button>
	</form>
</div>

<style>
	h1 {
		font-size: 1.5rem;
		margin-bottom: 1.5rem;
	}

	form {
		display: flex;
		flex-direction: column;
		gap: 1.25rem;
		max-width: 500px;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}

	label span {
		font-size: 0.85rem;
		font-weight: 600;
		color: var(--text-muted);
	}

	input, textarea, select {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.75rem;
		color: var(--text);
		font-size: 1rem;
	}

	input:focus, textarea:focus, select:focus {
		outline: none;
		border-color: var(--accent);
	}

	.kind-selector {
		display: flex;
		gap: 0.5rem;
	}

	.kind-selector button {
		flex: 1;
		padding: 0.5rem;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text-muted);
		font-size: 0.9rem;
	}

	.kind-selector button.active {
		border-color: var(--accent);
		color: var(--accent);
		background: var(--accent-soft);
	}

	button[type="submit"] {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.75rem;
		border-radius: var(--radius);
		font-weight: 600;
		font-size: 1rem;
		margin-top: 0.5rem;
	}

	button[type="submit"]:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.error {
		color: var(--critical);
		font-size: 0.85rem;
	}

	.image-previews {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.preview-item {
		position: relative;
		width: 72px;
		height: 72px;
	}

	.preview-item img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		border-radius: var(--radius-md);
		border: 1px solid var(--border);
	}

	.remove-img {
		position: absolute;
		top: -6px;
		right: -6px;
		background: var(--critical);
		color: var(--text-on-critical);
		border-radius: 50%;
		width: 20px;
		height: 20px;
		font-size: 12px;
		line-height: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		min-height: unset;
		min-width: unset;
	}

	.file-input {
		display: none;
	}

	.img-btn {
		font-size: var(--text-sm);
	}

	.location-picker {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.coords {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.85rem;
		color: var(--text);
		margin: 0;
	}

	.clear-pin {
		background: none;
		color: var(--accent);
		font-size: 0.8rem;
		font-weight: 600;
		padding: 0;
		min-height: unset;
		min-width: unset;
		text-decoration: underline;
		text-underline-offset: 2px;
	}

	.hint {
		font-size: 0.8rem;
		color: var(--text-muted);
		margin: 0;
	}
</style>
