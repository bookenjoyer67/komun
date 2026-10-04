<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { resendVerification, verifyEmail, getActiveAuth } from '$lib/stores/auth';

	let status = $state<'working' | 'done' | 'failed' | 'idle'>('idle');
	let error = $state('');
	let resendEmail = $state('');
	let resent = $state(false);

	onMount(async () => {
		resendEmail = getActiveAuth()?.email || '';
		const token = $page.url.searchParams.get('token');
		if (!token) return;

		status = 'working';
		const result = await verifyEmail(token);
		if (result.ok) {
			status = 'done';
		} else {
			status = 'failed';
			error = result.error || 'That link is invalid or has expired.';
		}
	});

	async function handleResend() {
		resent = false;
		error = '';
		if (!resendEmail.trim()) {
			error = 'Enter the address you signed up with.';
			return;
		}
		const result = await resendVerification(resendEmail);
		if (result.ok) {
			resent = true;
		} else {
			error = result.error || 'Could not send another link.';
		}
	}
</script>

<svelte:head><title>Verify your email · Komun</title></svelte:head>

<div class="container">
	<h1>Verify your email</h1>

	{#if status === 'working'}
		<p class="lede">Checking your link…</p>
	{:else if status === 'done'}
		<p class="ok">Your email address is verified.</p>
		<p class="alt"><a href="/account">Back to your account</a></p>
	{:else}
		{#if status === 'failed'}
			<p class="error">{error}</p>
		{:else}
			<p class="lede">
				Verification links last 24 hours and work once. If yours has gone stale, send
				yourself another.
			</p>
		{/if}

		<form onsubmit={(e) => { e.preventDefault(); handleResend(); }}>
			<label for="email">Email</label>
			<input id="email" type="email" autocomplete="email" bind:value={resendEmail} required />
			{#if resent}
				<!-- Same wording whether or not the address is on file, for the same anti-enumeration reason as the reset form. -->
				<p class="ok">If that address needs verifying, a new link is on its way.</p>
			{/if}
			{#if error && status !== 'failed'}<p class="error">{error}</p>{/if}
			<button type="submit" class="primary">Send another link</button>
		</form>

		<p class="alt"><a href="/account">Back to your account</a></p>
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
		font-family: inherit;
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

	.ok { color: var(--success); font-weight: 600; font-size: 0.95rem; }
	.error { color: var(--critical); font-size: 0.9rem; }
	.alt { margin-top: 1.25rem; font-size: 0.9rem; color: var(--text-muted); }
</style>
