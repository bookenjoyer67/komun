<script lang="ts">
	import {
		ALREADY_REVIEWED_MESSAGE,
		ReviewApiError,
		countChars,
		createReview,
		MAX_RATING,
		MAX_REVIEW_BODY,
		MIN_RATING,
		validateBody,
		validateRating
	} from '$lib/api/reviews';

	/**
	 * A review is writable only against a completed deal, so this modal is mounted only from a
	 * `completed` thread; a server 409 keeps its own sentence rather than a generic failure.
	 */
	interface Props {
		matchId: string;
		revieweeName?: string;
		onClose: () => void;
		onSubmitted?: () => void;
	}

	let { matchId, revieweeName, onClose, onSubmitted }: Props = $props();

	let rating = $state(0);
	let body = $state('');
	let error = $state('');
	let submitting = $state(false);
	let done = $state(false);
	let alreadyReviewed = $state(false);

	const stars = [1, 2, 3, 4, 5];
	const chars = $derived(countChars(body));

	async function handleSubmit() {
		const ratingError = validateRating(rating);
		if (ratingError) {
			error = ratingError;
			return;
		}
		const bodyError = validateBody(body);
		if (bodyError) {
			error = bodyError;
			return;
		}

		submitting = true;
		error = '';
		try {
			await createReview(matchId, { rating, body });
			done = true;
			onSubmitted?.();
		} catch (e) {
			if (e instanceof ReviewApiError) {
				error = e.message;
				if (e.status === 409 && e.message === ALREADY_REVIEWED_MESSAGE) {
					alreadyReviewed = true;
				}
			} else {
				error = e instanceof Error && e.message ? e.message : 'Could not post your review.';
			}
		}
		submitting = false;
	}
</script>

<div class="overlay" role="dialog" aria-modal="true" aria-label="Review this deal">
	<div class="modal">
		<button type="button" class="close-btn" onclick={onClose} aria-label="Close">&times;</button>

		{#if alreadyReviewed}
			<div class="state">
				<h2>Already reviewed</h2>
				<p>You have already reviewed this deal.</p>
				<button type="button" class="btn-secondary" onclick={onClose}>Close</button>
			</div>
		{:else if done}
			<div class="state">
				<h2>Thanks for the review</h2>
				<p>Your rating is now on {revieweeName ?? 'their'} profile.</p>
				<button type="button" class="btn-primary" onclick={onClose}>Close</button>
			</div>
		{:else}
			<h2>Review this deal</h2>
			{#if revieweeName}
				<p class="subtitle">How was your deal with {revieweeName}?</p>
			{/if}

			<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
				<fieldset class="rating">
					<legend>Rating</legend>
					{#each stars as star (star)}
						<label class="star">
							<input
								type="radio"
								name="rating"
								value={star}
								bind:group={rating}
								disabled={submitting}
							/>
							<span class="star-icon" aria-hidden="true">★</span>
							<span class="star-label">
								{star} {star === 1 ? 'star' : 'stars'}{#if star === MAX_RATING} (best){/if}
							</span>
						</label>
					{/each}
				</fieldset>

				<label class="body-label">
					<span>Review (optional)</span>
					<textarea
						bind:value={body}
						rows="4"
						maxlength={MAX_REVIEW_BODY}
						disabled={submitting}
					></textarea>
				</label>
				<p class="counter" data-testid="review-counter">{chars} / {MAX_REVIEW_BODY}</p>

				{#if error}
					<p class="error" role="alert">{error}</p>
				{/if}

				<button type="submit" class="btn-primary" disabled={submitting}>
					{submitting ? 'Posting...' : 'Post review'}
				</button>
			</form>
		{/if}
	</div>
</div>

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
		padding: 2rem;
		max-width: 460px;
		width: 100%;
		position: relative;
	}

	.close-btn {
		position: absolute;
		top: 0.75rem;
		right: 1rem;
		background: none;
		color: var(--text-muted);
		font-size: 1.5rem;
	}

	h2 {
		margin-bottom: 0.25rem;
		font-size: 1.2rem;
	}

	.subtitle {
		color: var(--text-muted);
		font-size: 0.85rem;
		margin-bottom: 1rem;
	}

	form {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.rating {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.5rem 0.75rem;
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem 0.75rem;
	}

	.rating legend {
		font-size: 0.8rem;
		font-weight: 600;
		color: var(--text-muted);
		padding: 0 0.25rem;
	}

	.star {
		display: flex;
		align-items: center;
		gap: 0.25rem;
		font-size: 0.85rem;
		cursor: pointer;
	}

	.star-icon {
		color: var(--warning, #f5a623);
	}

	.body-label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.body-label span {
		font-size: 0.8rem;
		font-weight: 600;
		color: var(--text-muted);
	}

	textarea {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.6rem;
		color: var(--text);
		font-size: 0.95rem;
		font-family: inherit;
		resize: vertical;
	}

	textarea:focus {
		outline: none;
		border-color: var(--accent);
	}

	.counter {
		text-align: right;
		font-size: 0.75rem;
		color: var(--text-muted);
		margin-top: -0.5rem;
	}

	.error {
		color: var(--critical);
		font-size: 0.85rem;
	}

	.btn-primary {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.6rem;
		border-radius: var(--radius);
		font-weight: 600;
		font-size: 0.95rem;
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.btn-secondary {
		background: var(--bg-elevated);
		color: var(--text);
		padding: 0.6rem;
		border-radius: var(--radius);
		font-weight: 600;
		font-size: 0.95rem;
		border: 1px solid var(--border);
	}

	.state {
		text-align: center;
		padding: 0.5rem 0;
	}

	.state p {
		color: var(--text-muted);
		margin-bottom: 1rem;
	}
</style>
