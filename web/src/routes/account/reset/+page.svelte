<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { confirmPasswordReset } from '$lib/stores/auth';
	import RecoveryCode from '../RecoveryCode.svelte';

	const MIN_PASSWORD = 12;

	const token = $derived($page.url.searchParams.get('token') || '');

	let password = $state('');
	let confirm = $state('');
	let recoveryInput = $state('');
	let acceptLoss = $state(false);
	let loading = $state(false);
	let error = $state('');
	let newRecoveryCode = $state('');
	let done = $state(false);

	const wordCount = $derived(recoveryInput.trim() ? recoveryInput.trim().split(/\s+/).length : 0);
	const hasCode = $derived(wordCount > 0);

	async function handleSubmit() {
		error = '';
		if (!token) {
			error = 'This link is missing its token. Request a new one.';
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
		if (hasCode && wordCount !== 12) {
			error = `A recovery code is 12 words; that is ${wordCount}.`;
			return;
		}
		if (!hasCode && !acceptLoss) {
			error = 'Tick the box to confirm you understand what resetting without the code costs.';
			return;
		}

		loading = true;
		const result = await confirmPasswordReset({
			token,
			password,
			recoveryCode: hasCode ? recoveryInput : undefined,
		});
		loading = false;

		if (!result.ok) {
			error = result.error || 'Could not reset the password.';
			return;
		}

		password = '';
		confirm = '';
		recoveryInput = '';
		if (result.recoveryCode) {
			newRecoveryCode = result.recoveryCode;
		} else {
			done = true;
		}
	}
</script>

<svelte:head><title>Choose a new password · Komun</title></svelte:head>

<div class="container">
	{#if newRecoveryCode}
		<p class="ok">
			Password reset. Your old encryption key could not be recovered, so a new one was created
			— along with the code below.
		</p>
		<RecoveryCode
			code={newRecoveryCode}
			doneLabel="Sign in"
			onDone={() => goto('/account/login')}
		/>
	{:else if done}
		<h1>Password reset</h1>
		<p class="ok">
			Your recovery code opened your existing encryption key, so everything sent to you before
			the reset is still readable.
		</p>
		<p class="lede">Every session that was signed in has been signed out, including any of yours.</p>
		<a class="primary link" href="/account/login">Sign in</a>
	{:else}
		<h1>Choose a new password</h1>

		{#if !token}
			<p class="error">
				This link is missing its token. <a href="/account/forgot">Request a new one.</a>
			</p>
		{/if}

		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
			<label for="password">New password</label>
			<input id="password" type="password" autocomplete="new-password" bind:value={password} required />

			<label for="confirm">Confirm new password</label>
			<input id="confirm" type="password" autocomplete="new-password" bind:value={confirm} required />

			<label for="recovery">Recovery code <span class="opt">(12 words, if you have it)</span></label>
			<textarea
				id="recovery"
				rows="3"
				autocomplete="off"
				spellcheck="false"
				placeholder="word word word …"
				bind:value={recoveryInput}
			></textarea>
			{#if hasCode}
				<p class="hint">{wordCount} of 12 words.</p>
			{/if}

			{#if !hasCode}
				<!-- The one irreversible choice in the flow: without the code nobody can open the old key bundle. -->
				<label class="accept">
					<input type="checkbox" bind:checked={acceptLoss} />
					I do not have my recovery code. I understand that messages sent to me before this
					reset will stay unreadable, permanently.
				</label>
			{/if}

			{#if error}<p class="error">{error}</p>{/if}

			<button type="submit" class="primary" disabled={loading || !token}>
				{loading ? 'Resetting…' : 'Reset password'}
			</button>
		</form>
	{/if}
</div>

<style>
	h1 { font-size: 1.5rem; margin-bottom: 0.4rem; }
	.lede { color: var(--text-muted); font-size: 0.9rem; margin-bottom: 1rem; max-width: 440px; }

	form { display: flex; flex-direction: column; gap: 0.35rem; max-width: 440px; }
	label { font-size: 0.85rem; font-weight: 600; margin-top: 0.6rem; }
	.opt { font-weight: 400; color: var(--text-muted); }

	input, textarea {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.7rem;
		color: var(--text);
		font-size: 1rem;
		font-family: inherit;
	}
	input:focus, textarea:focus { outline: none; border-color: var(--accent); }
	textarea { font-family: ui-monospace, monospace; font-size: 0.9rem; }

	.accept {
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
		font-weight: 400;
		font-size: 0.85rem;
		color: var(--text-muted);
		margin-top: 0.8rem;
	}
	.accept input { flex: none; margin-top: 0.15rem; }

	.primary {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.7rem;
		border-radius: var(--radius);
		font-weight: 600;
		margin-top: 1rem;
	}
	.primary:disabled { opacity: 0.6; }
	.link { display: inline-block; text-decoration: none; }

	.hint { font-size: 0.8rem; color: var(--text-muted); }
	.ok { color: var(--success); font-weight: 600; font-size: 0.95rem; margin-bottom: 0.75rem; max-width: 440px; }
	.error { color: var(--critical); font-size: 0.9rem; margin-top: 0.6rem; }
</style>
