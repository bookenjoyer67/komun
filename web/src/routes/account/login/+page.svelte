<script lang="ts">
	import { goto } from '$app/navigation';
	import { login } from '$lib/stores/auth';
	import { isConnected } from '$lib/stores/server';

	let email = $state('');
	let password = $state('');
	let deviceLabel = $state('');
	let loading = $state(false);
	let error = $state('');
	let warning = $state('');

	async function handleSubmit() {
		error = '';
		warning = '';
		if (!isConnected()) {
			error = 'Connect to a server first.';
			return;
		}
		if (!email.trim() || !password) {
			error = 'Enter your email and password.';
			return;
		}

		loading = true;
		const result = await login(email, password, deviceLabel.trim() || undefined);
		loading = false;

		if (!result.ok) {
			error = result.error || 'Could not sign in.';
			return;
		}

		password = '';
		// An `error` alongside `ok` means the key bundle did not open; warn now rather than let messages fail to decrypt later.
		if (result.error) {
			warning = result.error;
			return;
		}
		goto('/account');
	}
</script>

<svelte:head><title>Sign in · Komun</title></svelte:head>

<div class="container">
	<h1>Sign in</h1>
	<p class="lede">
		Signing in also unlocks your encryption key on this device. There is nothing else to type.
	</p>

	<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
		<label for="email">Email</label>
		<input id="email" type="email" autocomplete="email" bind:value={email} required />

		<label for="password">Password</label>
		<input
			id="password"
			type="password"
			autocomplete="current-password"
			bind:value={password}
			required
		/>

		<label for="device">Device name <span class="opt">(optional)</span></label>
		<input id="device" type="text" maxlength="60" placeholder="Laptop" bind:value={deviceLabel} />

		{#if error}<p class="error">{error}</p>{/if}
		{#if warning}
			<p class="warning">{warning}</p>
			<a class="alt-link" href="/account">Continue anyway</a>
		{/if}

		<button type="submit" class="primary" disabled={loading}>
			{loading ? 'Signing in…' : 'Sign in'}
		</button>
	</form>

	<p class="alt">
		<a href="/account/forgot">Forgot your password?</a>
	</p>
	<p class="alt">No account yet? <a href="/account/signup">Create one</a></p>
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

	.error { color: var(--critical); font-size: 0.9rem; margin-top: 0.6rem; }
	.warning { color: var(--text); background: var(--critical-soft); border-radius: var(--radius); padding: 0.6rem; font-size: 0.85rem; margin-top: 0.6rem; }
	.alt-link { font-size: 0.85rem; }
	.alt { margin-top: 1rem; font-size: 0.9rem; color: var(--text-muted); }
</style>
