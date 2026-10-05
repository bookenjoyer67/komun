<script lang="ts">
	/** The only time a recovery code is on screen: the server stores its ciphertext, never the code. */
	interface Props {
		code: string;
		email?: string;
		onDone: () => void;
		doneLabel?: string;
	}

	let { code, email = '', onDone, doneLabel = 'Continue' }: Props = $props();

	let confirmed = $state(false);
	let copied = $state(false);
	let downloaded = $state(false);

	const fileText = $derived(
		[
			'Komun recovery code',
			...(email ? [`Account: ${email}`] : []),
			'',
			code,
			'',
			'Keep this somewhere safe and offline.',
			'It is the only way back into your encrypted messages if you forget your password.',
			'Nobody — including the server — can recover it for you.',
			'',
		].join('\n')
	);

	function copy() {
		navigator.clipboard.writeText(code);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	function download() {
		const blob = new Blob([fileText], { type: 'text/plain' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = 'komun-recovery-code.txt';
		a.click();
		URL.revokeObjectURL(url);
		downloaded = true;
	}
</script>

<div class="recovery">
	<h2>Your recovery code</h2>
	<p class="lede">
		Write this down now. It is shown once and never again — not by us, not by the server.
	</p>

	<div class="code-grid">
		{#each code.split(' ') as word, i}
			<span class="word"><span class="num">{i + 1}</span>{word}</span>
		{/each}
	</div>

	<div class="actions">
		<button type="button" class="ghost" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
		<button type="button" class="ghost" onclick={download}>
			{downloaded ? 'Downloaded' : 'Download as file'}
		</button>
	</div>

	<p class="warn">
		If you forget your password and lose this code, your account can still be reset — but every
		message sent to you before the reset stays unreadable. There is no copy to fall back on.
	</p>

	<label class="confirm">
		<input type="checkbox" bind:checked={confirmed} />
		I have saved my recovery code somewhere safe
	</label>

	<button type="button" class="primary" disabled={!confirmed} onclick={onDone}>
		{doneLabel}
	</button>
</div>

<style>
	.recovery {
		max-width: 480px;
	}

	h2 {
		font-size: 1.2rem;
		margin-bottom: 0.4rem;
	}

	.lede {
		color: var(--text-muted);
		font-size: 0.9rem;
		margin-bottom: 1rem;
	}

	.code-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0.4rem;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.9rem;
		margin-bottom: 0.75rem;
	}

	.word {
		font-family: ui-monospace, monospace;
		font-size: 0.9rem;
		display: flex;
		gap: 0.4rem;
		align-items: baseline;
	}

	.num {
		color: var(--text-muted);
		font-size: 0.7rem;
		min-width: 1.1rem;
		text-align: right;
	}

	.actions {
		display: flex;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}

	.ghost {
		background: var(--bg-elevated);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.4rem 0.75rem;
		font-size: 0.85rem;
	}

	.warn {
		color: var(--text-muted);
		font-size: 0.85rem;
		border-left: 3px solid var(--critical);
		padding-left: 0.7rem;
		margin-bottom: 1rem;
	}

	.confirm {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		font-size: 0.9rem;
		margin-bottom: 1rem;
	}

	.primary {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.6rem 1rem;
		border-radius: var(--radius);
		font-weight: 600;
	}

	.primary:disabled {
		opacity: 0.5;
	}
</style>
