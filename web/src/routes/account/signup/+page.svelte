<script lang="ts">
	import { goto } from '$app/navigation';
	import { signup } from '$lib/stores/auth';
	import { isConnected } from '$lib/stores/server';
	import RecoveryCode from '../RecoveryCode.svelte';

	/** Mirrors the server's default `min_password_length`; the server enforces it regardless. */
	const MIN_PASSWORD = 12;

	let email = $state('');
	let displayName = $state('');
	let password = $state('');
	let confirm = $state('');
	let inviteCode = $state('');
	let loading = $state(false);
	let error = $state('');
	let recoveryCode = $state('');

	const tooShort = $derived(password.length > 0 && password.length < MIN_PASSWORD);
	const mismatch = $derived(confirm.length > 0 && confirm !== password);

	async function handleSubmit() {
		error = '';
		if (!isConnected()) {
			error = 'Connect to a server first.';
			return;
		}
		if (!email.trim() || !displayName.trim()) {
			error = 'Email and name are both required.';
			return;
		}
		if (password.length < MIN_PASSWORD) {
			error = `Password must be at least ${MIN_PASSWORD} characters.`;
			return;
		}
		if (password !== confirm) {
			error = 'Passwords do not match.';
			return;
		}

		loading = true;
		const result = await signup({
			email,
			displayName,
			password,
			inviteCode: inviteCode.trim() || undefined,
		});
		loading = false;

		if (!result.ok) {
			error = result.error || 'Could not create the account.';
			return;
		}

		// Clear the password from live component state: leaving it bound to an input is readable from the DOM.
		password = '';
		confirm = '';
		recoveryCode = result.recoveryCode || '';
		if (!recoveryCode) goto('/account');
	}
</script>

<svelte:head><title>Create an account · Komun</title></svelte:head>

<div class="container">
	{#if recoveryCode}
		<RecoveryCode
			code={recoveryCode}
			{email}
			doneLabel="Go to my account"
			onDone={() => goto('/account')}
		/>
	{:else}
		<h1>Create an account</h1>
		<p class="lede">
			Your password never leaves this device. It unlocks your encryption key here in the
			browser; the server only ever sees a value derived from it.
		</p>

		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
			<label for="email">Email</label>
			<input id="email" type="email" autocomplete="email" bind:value={email} required />

			<label for="name">Display name</label>
			<input id="name" type="text" maxlength="50" bind:value={displayName} required />

			<label for="password">Password</label>
			<input
				id="password"
				type="password"
				autocomplete="new-password"
				bind:value={password}
				required
			/>
			{#if tooShort}
				<p class="hint warn">At least {MIN_PASSWORD} characters.</p>
			{/if}

			<label for="confirm">Confirm password</label>
			<input
				id="confirm"
				type="password"
				autocomplete="new-password"
				bind:value={confirm}
				required
			/>
			{#if mismatch}
				<p class="hint warn">These do not match.</p>
			{/if}

			<label for="invite">Invite code <span class="opt">(if the server needs one)</span></label>
			<input id="invite" type="text" bind:value={inviteCode} />

			{#if error}<p class="error">{error}</p>{/if}

			<button type="submit" class="primary" disabled={loading}>
				{loading ? 'Creating account…' : 'Create account'}
			</button>
		</form>

		<p class="alt">Already have an account? <a href="/account/login">Sign in</a></p>
	{/if}
</div>

<style>
	h1 { font-size: 1.5rem; margin-bottom: 0.4rem; }
	.lede { color: var(--text-muted); font-size: 0.9rem; margin-bottom: 1.25rem; max-width: 420px; }

	form { display: flex; flex-direction: column; gap: 0.35rem; max-width: 400px; }
	label { font-size: 0.85rem; font-weight: 600; margin-top: 0.6rem; }
	.opt { font-weight: 400; color: var(--text-muted); }

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
	.primary:disabled { opacity: 0.6; }

	.hint { font-size: 0.8rem; color: var(--text-muted); }
	.warn { color: var(--critical); }
	.error { color: var(--critical); font-size: 0.9rem; margin-top: 0.6rem; }
	.alt { margin-top: 1.25rem; font-size: 0.9rem; color: var(--text-muted); }
</style>
