<script lang="ts">
	import { requestPasswordReset } from '$lib/stores/auth';
	import { isConnected } from '$lib/stores/server';

	let email = $state('');
	let loading = $state(false);
	let sent = $state(false);
	let error = $state('');

	async function handleSubmit() {
		error = '';
		if (!isConnected()) {
			error = 'Connect to a server first.';
			return;
		}
		if (!email.trim()) {
			error = 'Enter your email address.';
			return;
		}
		loading = true;
		const result = await requestPasswordReset(email);
		loading = false;
		if (!result.ok) {
			error = result.error || 'Could not send the reset mail.';
			return;
		}
		sent = true;
	}
</script>

<svelte:head><title>Reset your password · Komun</title></svelte:head>

<div class="container">
	<h1>Reset your password</h1>

	{#if sent}
		<!-- Same message whether or not the address exists: naming a missing account is a membership check. -->
		<p class="ok">
			If that address has an account here, a reset link is on its way. The link is good for
			30 minutes and can be used once.
		</p>
		<p class="lede">
			Have your recovery code to hand when you follow it. Without the code your account comes
			back, but messages sent to you before the reset stay encrypted to a key nobody holds.
		</p>
		<p class="alt"><a href="/account/login">Back to sign in</a></p>
	{:else}
		<p class="lede">
			We will email you a one-time link. Nothing about your account changes until you follow it.
		</p>

		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
			<label for="email">Email</label>
			<input id="email" type="email" autocomplete="email" bind:value={email} required />

			{#if error}<p class="error">{error}</p>{/if}

			<button type="submit" class="primary" disabled={loading}>
				{loading ? 'Sending…' : 'Send reset link'}
			</button>
		</form>

		<p class="alt"><a href="/account/login">Back to sign in</a></p>
	{/if}
</div>

<style>
	h1 { font-size: 1.5rem; margin-bottom: 0.4rem; }
	.lede { color: var(--text-muted); font-size: 0.9rem; margin-bottom: 1.25rem; max-width: 440px; }

	form { display: flex; flex-direction: column; gap: 0.35rem; max-width: 400px; }
	label { font-size: 0.85rem; font-weight: 600; margin-top: 0.6rem; }

	input {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.7rem;
		color: var(--text);
		font-size: 1rem;
	}
	input:focus { outline: none; border-color: var(--accent); }

	.primary {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.7rem;
		border-radius: var(--radius);
		font-weight: 600;
		margin-top: 1rem;
	}
	.primary:disabled { opacity: 0.6; }

	.ok { color: var(--success); font-weight: 600; font-size: 0.95rem; margin-bottom: 0.75rem; max-width: 440px; }
	.error { color: var(--critical); font-size: 0.9rem; margin-top: 0.6rem; }
	.alt { margin-top: 1.25rem; font-size: 0.9rem; color: var(--text-muted); }
</style>
