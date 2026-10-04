<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { requireAuth } from '$lib/stores/auth';
	import LocationMap from '$lib/components/LocationMap.svelte';
	import {
		createMarketPost,
		ITEM_CONDITIONS,
		MARKET_KIND_LABELS,
		parsePriceToCents,
		type ItemCondition,
		type MarketKind
	} from '$lib/api/market';
	import { fetchMarketCategories, type Category } from '$lib/api/categories';

	let kind = $state<MarketKind>('listing');
	let title = $state('');
	let body = $state('');
	let category = $state('');
	let priceText = $state('');
	let currency = $state('');
	let negotiable = $state(false);
	let condition = $state<ItemCondition | ''>('good');
	let expiresIn = $state('14');
	let locationName = $state('');
	let locationLat: number | null = $state(null);
	let locationLon: number | null = $state(null);
	let contactMethod = $state('');

	// Categories come from the seeded table; there is no hardcoded option list.
	let categories = $state<Category[]>([]);
	let categoryError = $state('');
	let error = $state('');
	let loading = $state(false);

	const mapCenter = { lat: 20, lon: 0, zoom: 2 };

	onMount(async () => {
		try {
			categories = await fetchMarketCategories();
			if (!category && categories.length > 0) category = categories[0].slug;
		} catch (e) {
			categoryError = e instanceof Error ? e.message : 'Could not load categories';
		}
	});

	function setPickedLocation(coords: { lat: number; lon: number }) {
		locationLat = coords.lat;
		locationLon = coords.lon;
	}

	function clearLocation() {
		locationLat = null;
		locationLon = null;
	}

	function getExpiresAt(): string | null {
		const days = parseInt(expiresIn);
		if (!days) return null;
		return new Date(Date.now() + days * 86400000).toISOString();
	}

	function onCondition(e: Event) {
		condition = (e.currentTarget as HTMLSelectElement).value as ItemCondition | '';
	}

	function submit() {
		requireAuth(async () => {
			error = '';

			if (!title.trim()) {
				error = 'Title is required';
				return;
			}
			if (!category) {
				error = 'Pick a category';
				return;
			}
			if (String(priceText).trim() && parsePriceToCents(priceText) === null) {
				error = 'Price must be a non-negative number';
				return;
			}

			loading = true;
			try {
				const post = await createMarketPost({
					kind,
					category,
					title: title.trim(),
					body: body.trim() || null,
					price_cents: parsePriceToCents(priceText),
					// Empty stays empty: the server resolves `[market] default_currency`; the client must not invent one.
					currency: currency.trim() || null,
					price_negotiable: negotiable,
					item_condition: condition || null,
					expires_at: getExpiresAt(),
					location_name: locationName.trim() || null,
					location_lat: locationLat,
					location_lon: locationLon,
					contact_method: contactMethod.trim() || null
				});
				goto(`/p/${post.id}`);
			} catch (e) {
				error = e instanceof Error ? e.message : 'Failed to create the post';
			} finally {
				loading = false;
			}
		});
	}
</script>

<svelte:head>
	<title>New market post — Komun</title>
</svelte:head>

<div class="container">
	<h1>Post to the market</h1>

	<form onsubmit={(e) => { e.preventDefault(); submit(); }}>
		<label>
			<span>Type</span>
			<div class="kind-selector">
				<button
					type="button"
					class:active={kind === 'listing'}
					onclick={() => (kind = 'listing')}
				>
					{MARKET_KIND_LABELS.listing}
				</button>
				<button type="button" class:active={kind === 'want'} onclick={() => (kind = 'want')}>
					{MARKET_KIND_LABELS.want}
				</button>
			</div>
			<small id="kind-hint">
				{kind === 'listing' ? 'Something you are offering for sale.' : 'Something you are looking for.'}
			</small>
		</label>

		<label>
			<span>Category</span>
			<select
				value={category}
				onchange={(e) => (category = (e.currentTarget as HTMLSelectElement).value)}
				disabled={categories.length === 0}
			>
				{#if categories.length === 0}
					<option value="">Loading categories...</option>
				{/if}
				{#each categories as option}
					<option value={option.slug}>{option.label}</option>
				{/each}
			</select>
		</label>
		{#if categoryError}
			<p class="error">{categoryError}</p>
		{/if}

		<label>
			<span>Title</span>
			<input
				type="text"
				bind:value={title}
				placeholder={kind === 'listing' ? 'What are you selling?' : 'What are you looking for?'}
				maxlength="200"
			/>
		</label>

		<label>
			<span>Details (optional)</span>
			<textarea bind:value={body} placeholder="More info..." rows="3"></textarea>
		</label>

		<div class="row">
			<label>
				<span>Price (optional)</span>
				<input type="number" min="0" step="0.01" bind:value={priceText} placeholder="0.00" />
			</label>

			<label>
				<span>Currency (optional)</span>
				<input
					type="text"
					bind:value={currency}
					placeholder="USD"
					maxlength="3"
					aria-describedby="currency-hint"
				/>
			</label>
		</div>
		<small id="currency-hint">
			Leave the currency blank to use this server's default. Leave the price blank for “free /
			negotiable”.
		</small>

		<label class="checkbox">
			<input type="checkbox" bind:checked={negotiable} />
			<span>Price is negotiable</span>
		</label>

		<label>
			<span>Condition</span>
			<select value={condition} onchange={onCondition}>
				<option value="">Unspecified</option>
				{#each ITEM_CONDITIONS as option}
					<option value={option.value}>{option.label}</option>
				{/each}
			</select>
		</label>

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
			<span>Location name (optional)</span>
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
			<p class="error" role="alert">{error}</p>
		{/if}

		<button type="submit" disabled={loading || categories.length === 0}>
			{loading ? 'Posting...' : 'Post to market'}
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

	label.checkbox {
		flex-direction: row;
		align-items: center;
		gap: 0.5rem;
	}

	label.checkbox span {
		font-size: 0.9rem;
		color: var(--text);
	}

	input,
	textarea,
	select {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.75rem;
		color: var(--text);
		font-size: 1rem;
		font-family: inherit;
	}

	input:focus,
	textarea:focus,
	select:focus {
		outline: none;
		border-color: var(--accent);
	}

	input[type='checkbox'] {
		width: auto;
		align-self: flex-start;
	}

	.row {
		display: flex;
		gap: 0.75rem;
	}

	.row label {
		flex: 1;
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

	small {
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	button[type='submit'] {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.75rem;
		border-radius: var(--radius);
		font-weight: 600;
		font-size: 1rem;
		margin-top: 0.5rem;
	}

	button[type='submit']:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.error {
		color: var(--critical);
		font-size: 0.85rem;
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
