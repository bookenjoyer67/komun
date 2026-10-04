<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { isConnected, getActiveServer } from '$lib/stores/server';
	import { isAuthenticated, auth, getEncryptionSecretKey } from '$lib/stores/auth';
	import { api } from '$lib/api/client';
	import { deriveConversationKey, encryptMessage, decryptMessage } from '$lib/crypto';
	import OfferPanel from '$lib/components/OfferPanel.svelte';
	import DealReviewModal from '$lib/components/DealReviewModal.svelte';
	import { listOffers, type Offer } from '$lib/api/offers';

	interface Message {
		id: string;
		sender_id: string;
		body: string;
		created_at: string;
	}

	interface DecryptedMessage {
		id: string;
		sender_id: string;
		/** Decrypted plaintext; always empty when `undecryptable` is set. */
		body: string;
		created_at: string;
		encrypted: boolean;
		/** True when the body could not be opened with the shared key. The raw body is dropped. */
		undecryptable: boolean;
	}

	interface Conversation {
		match_id: string;
		post_id: string;
		post_title: string;
		post_kind: string;
		responder_id: string;
		author_id: string;
		responder_name: string;
		author_name: string;
		status: string;
		messages: Message[];
	}

	/**
	 * The one string an undecryptable message ever renders as. The server can put anything in a
	 * message body, so a body that does not open under the shared key is never shown: showing it
	 * would let the server put words in the other party's mouth (VC17).
	 */
	const UNDECRYPTABLE_PLACEHOLDER = 'Could not decrypt this message.';

	let convo = $state<Conversation | null>(null);
	let decryptedMessages: DecryptedMessage[] = $state([]);
	let newMessage = $state('');
	let loading = $state(true);
	let sending = $state(false);
	let error = $state('');
	let sendError = $state('');
	let sharedKey: string | null = $state(null);
	let encrypted = $state(false);
	/**
	 * Why there is no shared key yet. Each specific reason is worded exactly as `RespondModal`'s
	 * `seal()` words the same failure; only the last-resort catch-all has no counterpart there.
	 */
	let keyError = $state('');
	/** The negotiation trail, fetched for market threads only (see `loadConversation`). */
	let offers: Offer[] = $state([]);
	let showReview = $state(false);
	let statusError = $state('');

	let myUserId = $derived((() => {
		const server = getActiveServer();
		if (!server) return null;
		return $auth.servers?.[server]?.userId || null;
	})());

	/** Offers are for listings and wanted ads; an aid thread keeps its plain propose/accept flow. */
	const isMarket = $derived(convo?.post_kind === 'listing' || convo?.post_kind === 'want');
	const names = $derived<Record<string, string>>(
		convo ? { [convo.author_id]: convo.author_name, [convo.responder_id]: convo.responder_name } : {}
	);

	onMount(() => {
		if (!isConnected() || !isAuthenticated()) {
			goto('/');
			return;
		}
		loadConversation();
		const interval = setInterval(loadConversation, 5000);
		return () => clearInterval(interval);
	});

	async function loadConversation() {
		const matchId = $page.params.id as string;
		if (!matchId) return;
		try {
			const loaded = await api.conversations.get(matchId);
			convo = loaded;
			// A market thread carries a negotiation; an aid thread has none, and asking for one
			// would be a 400 the page has no use for.
			if (loaded.post_kind === 'listing' || loaded.post_kind === 'want') {
				offers = await listOffers(matchId);
			} else {
				offers = [];
			}
			await setupEncryption();
			await decryptMessages();
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	/**
	 * Derive the conversation key, or record in `keyError` why it cannot be derived. A failure here
	 * never downgrades anything: without a key the page cannot send (see `seal`) and renders every
	 * message as the fixed placeholder (see `decryptMessages`).
	 */
	async function setupEncryption() {
		if (!convo || sharedKey) return;
		const mySecret = getEncryptionSecretKey();
		if (!mySecret) {
			keyError = 'Your encryption key is locked — sign in again to unlock it';
			return;
		}

		const otherPartyId = myUserId === convo.author_id ? convo.responder_id : convo.author_id;
		try {
			const server = getActiveServer();
			const res = await fetch(`${server}/api/auth/users/${otherPartyId}/keys`);
			if (!res.ok) {
				keyError = 'Could not fetch the recipient\'s encryption key';
				return;
			}
			const keys = await res.json();
			if (!keys.encryption_public_key) {
				keyError = 'The recipient has no encryption key yet';
				return;
			}

			sharedKey = await deriveConversationKey(mySecret, keys.encryption_public_key);
			encrypted = true;
			keyError = '';
		} catch {
			keyError = 'Could not set up encryption for this conversation';
		}
	}

	async function decryptMessages() {
		if (!convo) return;
		const msgs: DecryptedMessage[] = [];
		for (const msg of convo.messages) {
			// The raw body is never copied into the rendered list: either it opens under the shared
			// key, or the message becomes the placeholder.
			const shell = { id: msg.id, sender_id: msg.sender_id, created_at: msg.created_at };
			if (encrypted && sharedKey) {
				try {
					const plaintext = await decryptMessage(msg.body, sharedKey);
					msgs.push({ ...shell, body: plaintext, encrypted: true, undecryptable: false });
				} catch {
					msgs.push({ ...shell, body: '', encrypted: false, undecryptable: true });
				}
			} else {
				msgs.push({ ...shell, body: '', encrypted: false, undecryptable: true });
			}
		}
		decryptedMessages = msgs;
	}

	/**
	 * The thread's counterpart of `RespondModal`'s `seal()`, with the same contract: it returns
	 * ciphertext produced under the shared key, or it throws. There is deliberately no plaintext
	 * fallback (VC13, D12). A missing key is retried once here, because the key fetch can fail
	 * transiently; if it is still missing, the reason `setupEncryption` recorded is thrown.
	 */
	async function seal(plaintext: string): Promise<string> {
		if (!sharedKey) await setupEncryption();
		const key = sharedKey;
		if (!encrypted || !key) {
			throw new Error(keyError || 'This conversation has no shared encryption key');
		}
		const ciphertext = await encryptMessage(plaintext, key);
		if (!ciphertext || ciphertext === plaintext) {
			throw new Error('Could not encrypt this message');
		}
		return ciphertext;
	}

	async function sendMessage() {
		if (!newMessage.trim() || !convo) return;
		const matchId = convo.match_id;
		sending = true;
		sendError = '';

		// Seal first, in its own step: any failure, including one inside the encrypt call, ends
		// here with a visible error and no request. The draft stays in the box.
		let ciphertext: string;
		try {
			ciphertext = await seal(newMessage.trim());
		} catch (e: any) {
			sendError = `Not sent: ${e?.message || 'could not encrypt this message'}. Messages are never sent unencrypted.`;
			sending = false;
			return;
		}

		try {
			await api.conversations.sendMessage(matchId, ciphertext);
			newMessage = '';
			await loadConversation();
		} catch (e: any) {
			sendError = e?.message || 'Could not send this message.';
		}
		sending = false;
	}

	/**
	 * The deal lifecycle behind `PATCH /api/conversations/{id}/status`. The server names the
	 * current status in a 409, so the sentence is shown verbatim rather than swallowed: it is the
	 * one fact that tells the viewer whether the deal moved under them.
	 */
	async function setStatus(status: string) {
		if (!convo) return;
		statusError = '';
		try {
			await api.conversations.updateStatus(convo.match_id, status);
			await loadConversation();
		} catch (e: any) {
			statusError = e.message || 'Could not update this conversation.';
		}
	}

	function formatTime(dateStr: string): string {
		return new Date(dateStr).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
	}
</script>

<div class="container">
	{#if loading}
		<p class="status">Loading...</p>
	{:else if error && !convo}
		<p class="status err">{error}</p>
	{:else if convo}
		<header class="thread-header">
			<a href="/messages" class="back">&larr;</a>
			<div class="header-info">
				<h2>{convo.post_title}</h2>
				<span class="participants">
					{convo.author_name} &harr; {convo.responder_name}
				</span>
			</div>
			<div class="header-right">
				{#if encrypted}
					<span class="lock" title="End-to-end encrypted">&#x1f512;</span>
				{/if}
				<span class="status-badge status-{convo.status}">{convo.status}</span>
			</div>
		</header>

		<div class="messages">
			{#each decryptedMessages as msg}
				<div
					class="bubble"
					class:mine={msg.sender_id === myUserId}
					class:theirs={msg.sender_id !== myUserId}
					class:undecryptable={msg.undecryptable}
				>
					{#if msg.undecryptable}
						<p class="undecryptable-text">&#x26a0; {UNDECRYPTABLE_PLACEHOLDER}</p>
					{:else}
						<p>{msg.body}</p>
					{/if}
					<span class="msg-time">
						{formatTime(msg.created_at)}
						{#if msg.encrypted}
							<span class="lock-small">&#x1f512;</span>
						{/if}
					</span>
				</div>
			{/each}
		</div>

		{#if isMarket}
			<OfferPanel
				matchId={convo.match_id}
				postKind={convo.post_kind}
				status={convo.status}
				{myUserId}
				{offers}
				{names}
				onchange={loadConversation}
			/>
		{/if}

		{#if convo.status === 'completed'}
			<p class="deal-done">This deal is complete.</p>
			<button class="review-btn" onclick={() => (showReview = true)}>Leave a review</button>
		{/if}

		{#if convo.status === 'proposed' || convo.status === 'accepted'}
			<div class="deal-controls">
				{#if convo.status === 'accepted'}
					<button class="complete-btn" onclick={() => setStatus('completed')}>
						Mark as completed
					</button>
				{/if}
				{#if !isMarket && convo.status === 'proposed' && myUserId === convo.author_id}
					<button class="accept-btn" onclick={() => setStatus('accepted')}>Accept</button>
				{/if}
				<button class="withdraw-btn" onclick={() => setStatus('withdrawn')}>Withdraw</button>
			</div>
		{/if}

		{#if statusError}
			<p class="err" role="alert">{statusError}</p>
		{/if}

		{#if convo.status !== 'completed' && convo.status !== 'withdrawn'}
			{#if !encrypted && keyError}
				<p class="key-warning" role="status">
					Messages in this conversation cannot be sent until encryption is available: {keyError}.
				</p>
			{/if}
			{#if sendError}
				<p class="err send-err" role="alert">{sendError}</p>
			{/if}
			<form class="send-form" onsubmit={(e) => { e.preventDefault(); sendMessage(); }}>
				<input
					type="text"
					bind:value={newMessage}
					placeholder="Type a message..."
					disabled={sending}
				/>
				<button type="submit" disabled={sending || !newMessage.trim()}>Send</button>
			</form>
		{:else if convo.status === 'withdrawn'}
			<p class="closed">This conversation has been marked as {convo.status}.</p>
		{/if}
	{/if}
</div>

{#if showReview && convo}
	<DealReviewModal
		matchId={convo.match_id}
		revieweeName={myUserId === convo.author_id ? convo.responder_name : convo.author_name}
		onClose={() => (showReview = false)}
		onSubmitted={() => {
			showReview = false;
			loadConversation();
		}}
	/>
{/if}

<style>
	.thread-header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin-bottom: 1.5rem;
		padding-bottom: 1rem;
		border-bottom: 1px solid var(--border);
	}

	.back { font-size: 1.2rem; color: var(--text-muted); }
	.header-info { flex: 1; }
	h2 { font-size: 1.1rem; }
	.participants { font-size: 0.8rem; color: var(--text-muted); }

	.header-right { display: flex; align-items: center; gap: 0.5rem; }
	.lock { font-size: 0.9rem; }

	.status-badge {
		font-size: 0.7rem;
		padding: 0.2rem 0.5rem;
		border-radius: 4px;
		text-transform: uppercase;
		font-weight: 600;
	}

	.status-proposed { background: var(--warning-soft); color: var(--warning); }
	.status-accepted { background: var(--success-soft); color: var(--success); }
	.status-completed { background: var(--success-strong); color: var(--success); }

	.messages {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		margin-bottom: 1.5rem;
		min-height: 200px;
		max-height: 60vh;
		overflow-y: auto;
		padding: 0.5rem 0;
	}

	.bubble {
		max-width: 75%;
		padding: 0.6rem 0.9rem;
		border-radius: var(--radius-lg);
		font-size: 0.9rem;
	}

	.bubble.mine {
		align-self: flex-end;
		background: var(--accent);
		color: var(--text-on-accent);
	}

	.bubble.theirs {
		align-self: flex-start;
		background: var(--bg-elevated);
		color: var(--text);
	}

	/* Declared after .mine and .theirs so it wins at equal specificity: an undecryptable message
	   must never look like a normal one. */
	.bubble.undecryptable {
		background: transparent;
		color: var(--text-muted);
		border: 1px dashed var(--critical);
	}

	.bubble p { margin: 0; word-break: break-word; }
	.bubble p.undecryptable-text { font-style: italic; }

	.msg-time {
		display: block;
		font-size: 0.65rem;
		opacity: 0.7;
		margin-top: 0.2rem;
	}

	.lock-small { font-size: 0.6rem; }

	.key-warning {
		color: var(--warning);
		font-size: 0.85rem;
		margin-bottom: 0.5rem;
	}

	.send-err {
		font-size: 0.85rem;
		margin-bottom: 0.5rem;
	}

	.send-form {
		display: flex;
		gap: 0.5rem;
	}

	input {
		flex: 1;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.75rem;
		color: var(--text);
		font-size: 1rem;
	}

	input:focus { outline: none; border-color: var(--accent); }

	button[type="submit"] {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.75rem 1.2rem;
		border-radius: var(--radius);
		font-weight: 600;
	}

	button[type="submit"]:disabled { opacity: 0.5; cursor: not-allowed; }

	.deal-controls {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 1rem;
	}

	.deal-controls button,
	.review-btn {
		padding: 0.6rem 1rem;
		border-radius: var(--radius);
		font-weight: 600;
	}

	.complete-btn,
	.accept-btn {
		background: var(--success);
		color: var(--text-on-success);
	}

	.withdraw-btn {
		background: var(--bg-surface);
		color: var(--critical);
		border: 1px solid var(--critical);
	}

	.review-btn {
		background: var(--accent);
		color: var(--text-on-accent);
		margin-top: 0.5rem;
	}

	.deal-done {
		text-align: center;
		color: var(--success);
		font-weight: 600;
		padding: 0.75rem 0 0;
	}

	.closed {
		text-align: center;
		color: var(--text-muted);
		padding: 1rem;
		font-style: italic;
	}

	.status { text-align: center; color: var(--text-muted); padding: 3rem 0; }
	.err { color: var(--critical); }

	@media (max-width: 480px) {
		.bubble { max-width: 85%; }
		.thread-header { flex-wrap: wrap; }
		h2 { font-size: 1rem; }
		.send-form { position: sticky; bottom: 0; background: var(--bg); padding: 0.5rem 0; }
	}
</style>
