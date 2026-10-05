<script lang="ts">
	import { formatPrice, parsePriceToCents } from '$lib/api/market';
	import {
		agreedOffer,
		canAccept,
		createOffer,
		lastNumberedOffer,
		nextOfferKind,
		OFFER_KIND_LABELS,
		type Offer,
		type OfferKind
	} from '$lib/api/offers';

	/**
	 * The negotiation half of a market thread; the page owns the `GET` and passes the ordered rows in.
	 *
	 * `postKind` keeps offers off an aid thread — the component renders nothing for anything else,
	 * and the server refuses the same request with a 400.
	 */
	interface Props {
		matchId: string;
		postKind: string;
		status: string;
		myUserId: string | null;
		offers?: Offer[];
		names?: Record<string, string>;
		onchange?: () => void;
	}

	let { matchId, postKind, status, myUserId, offers = [], names = {}, onchange }: Props = $props();

	let amountText = $state('');
	let noteText = $state('');
	let error = $state('');
	let busy = $state(false);

	const isMarket = $derived(postKind === 'listing' || postKind === 'want');
	/** A thread that is over takes no new step: `check_offer_allowed` refuses it server-side too. */
	const live = $derived(status === 'proposed' || status === 'accepted');
	const lastNumber = $derived(lastNumberedOffer(offers));
	const agreed = $derived(agreedOffer(offers));
	/** Accept is the counterparty's move, and only while there is still something to agree to. */
	const mayAccept = $derived(status === 'proposed' && canAccept(offers, myUserId));
	const primaryLabel = $derived(nextOfferKind(offers) === 'counter' ? 'Counter' : 'Make an offer');

	function actorName(id: string): string {
		if (myUserId && id === myUserId) return 'You';
		return names[id] ?? 'Participant';
	}

	function kindLabel(kind: OfferKind): string {
		return OFFER_KIND_LABELS[kind] ?? kind;
	}

	function message(e: unknown): string {
		return e instanceof Error && e.message ? e.message : 'Something went wrong.';
	}

	async function submitNumber() {
		const cents = parsePriceToCents(amountText);
		if (cents == null) {
			error = 'Enter an amount, for example 25.00.';
			return;
		}

		busy = true;
		error = '';
		try {
			await createOffer(matchId, {
				kind: nextOfferKind(offers),
				amount_cents: cents,
				note: noteText
			});
			amountText = '';
			noteText = '';
			onchange?.();
		} catch (e) {
			// A 409 carries the server's own sentence, which names the stale status and tells the viewer whether to reload or stop.
			error = message(e);
		}
		busy = false;
	}

	async function acceptNumber() {
		if (!lastNumber || lastNumber.amount_cents == null) {
			error = 'There is no offer on this conversation to accept.';
			return;
		}

		busy = true;
		error = '';
		try {
			await createOffer(matchId, {
				kind: 'accept',
				amount_cents: lastNumber.amount_cents,
				currency: lastNumber.currency,
				note: noteText
			});
			noteText = '';
			onchange?.();
		} catch (e) {
			error = message(e);
		}
		busy = false;
	}

	async function declineOffer() {
		busy = true;
		error = '';
		try {
			await createOffer(matchId, { kind: 'decline', note: noteText });
			noteText = '';
			onchange?.();
		} catch (e) {
			error = message(e);
		}
		busy = false;
	}
</script>

{#if isMarket}
	<section class="offer-panel" aria-label="Negotiation">
		<h3>Offers</h3>

		{#if offers.length === 0}
			<p class="empty">No offers yet.</p>
		{:else}
			<ol class="offer-list">
				{#each offers as offer (offer.id)}
					<li class="offer offer-{offer.kind}">
						<div class="offer-head">
							<span class="offer-kind">{kindLabel(offer.kind)}</span>
							{#if offer.amount_cents != null}
								<span class="offer-amount">{formatPrice(offer.amount_cents, offer.currency)}</span>
							{/if}
							<span class="offer-actor">{actorName(offer.actor_id)}</span>
						</div>
						{#if offer.note}
							<p class="offer-note">{offer.note}</p>
						{/if}
						<time class="offer-time" datetime={offer.created_at}>
							{new Date(offer.created_at).toLocaleString()}
						</time>
					</li>
				{/each}
			</ol>
		{/if}

		{#if agreed?.amount_cents != null}
			<p class="agreed">
				Agreed price: <strong>{formatPrice(agreed.amount_cents, agreed.currency)}</strong>
			</p>
		{/if}

		{#if live}
			<div class="offer-form">
				<label>
					<span>{primaryLabel === 'Counter' ? 'Your counter' : 'Your offer'}</span>
					<input
						type="text"
						inputmode="decimal"
						bind:value={amountText}
						placeholder="0.00"
						disabled={busy}
					/>
				</label>
				<label>
					<span>Note (optional)</span>
					<input type="text" bind:value={noteText} maxlength="500" disabled={busy} />
				</label>

				<div class="actions">
					<button type="button" class="primary" onclick={submitNumber} disabled={busy}>
						{primaryLabel}
					</button>
					{#if mayAccept}
						<button type="button" class="accept" onclick={acceptNumber} disabled={busy}>
							Accept
						</button>
					{/if}
					<button type="button" class="decline" onclick={declineOffer} disabled={busy}>
						Decline
					</button>
				</div>
			</div>
		{/if}

		{#if error}
			<p class="offer-error" role="alert">{error}</p>
		{/if}
	</section>
{/if}

<style>
	.offer-panel {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: var(--space-3, 0.75rem);
		margin-bottom: 1rem;
	}

	h3 {
		font-size: 0.95rem;
		margin-bottom: 0.5rem;
	}

	.empty {
		color: var(--text-muted);
		font-size: 0.85rem;
		font-style: italic;
	}

	.offer-list {
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		margin: 0 0 0.75rem;
	}

	.offer {
		background: var(--bg-elevated);
		border-left: 3px solid var(--border);
		border-radius: var(--radius);
		padding: 0.4rem 0.6rem;
	}

	.offer-accept {
		border-left-color: var(--success);
	}

	.offer-decline {
		border-left-color: var(--critical);
	}

	.offer-head {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
		font-size: 0.8rem;
	}

	.offer-kind {
		font-weight: 700;
		text-transform: uppercase;
		font-size: 0.65rem;
		color: var(--text-muted);
	}

	.offer-amount {
		font-weight: 700;
	}

	.offer-actor {
		margin-left: auto;
		color: var(--text-muted);
	}

	.offer-note {
		font-size: 0.85rem;
		margin: 0.2rem 0 0;
		word-break: break-word;
	}

	.offer-time {
		display: block;
		font-size: 0.65rem;
		color: var(--text-muted);
		margin-top: 0.15rem;
	}

	.agreed {
		font-size: 0.9rem;
		margin-bottom: 0.75rem;
	}

	.offer-form {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
	}

	label span {
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--text-muted);
	}

	input {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.5rem;
		color: var(--text);
		font-size: 0.95rem;
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	button {
		padding: 0.5rem 0.9rem;
		border-radius: var(--radius);
		font-weight: 600;
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.primary {
		background: var(--accent);
		color: var(--text-on-accent);
	}

	.accept {
		background: var(--success);
		color: var(--text-on-success);
	}

	.decline {
		background: var(--bg-surface);
		color: var(--critical);
		border: 1px solid var(--critical);
	}

	.offer-error {
		color: var(--critical);
		font-size: 0.85rem;
		margin-top: 0.5rem;
	}
</style>
