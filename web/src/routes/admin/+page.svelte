<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getToken, isSuperadmin } from '$lib/stores/auth';
	import { getActiveServer, isConnected } from '$lib/stores/server';
	import { api } from '$lib/api/client';

	/** `status` is the stored word; the page shows DECIDED_LABELS instead. */
	interface AppealItem {
		id: string;
		post_id: string;
		post_title: string;
		author_name: string;
		reason: string;
		body: string;
		status: string;
		admin_notes: string | null;
		created_at: string;
	}

	const DECIDED_LABELS: Record<string, string> = {
		granted: 'Post restored',
		denied: 'Appeal declined'
	};

	let stats: any = $state(null);
	let loading = $state(true);

	let appeals = $state<AppealItem[]>([]);
	let appealsState = $state<'loading' | 'ready' | 'error'>('loading');
	let notes = $state<Record<string, string>>({});
	let decisionErrors = $state<Record<string, string>>({});
	let deciding = $state<string | null>(null);

	const waiting = $derived(appeals.filter((a) => a.status === 'pending'));
	const decided = $derived(appeals.filter((a) => a.status !== 'pending'));

	onMount(() => {
		if (!isConnected() || !isSuperadmin()) { goto('/'); return; }
		loadStats();
		loadAppeals();
	});

	async function loadStats() {
		try { stats = await api.admin.stats(); } catch {}
		loading = false;
	}

	async function loadAppeals() {
		try {
			const res = await fetch(`${getActiveServer()}/api/admin/appeals`, {
				headers: { Authorization: `Bearer ${getToken()}` }
			});
			if (!res.ok) throw new Error(res.statusText);
			appeals = await res.json();
			appealsState = 'ready';
		} catch {
			appealsState = 'error';
		}
	}

	function setDecisionError(id: string, message: string) {
		decisionErrors = { ...decisionErrors, [id]: message };
	}

	async function decide(appeal: AppealItem, status: 'granted' | 'denied') {
		const note = (notes[appeal.id] ?? '').trim();
		if (status === 'denied' && !note) {
			setDecisionError(appeal.id, 'Write a note to the author before declining.');
			return;
		}
		deciding = appeal.id;
		setDecisionError(appeal.id, '');
		try {
			const res = await fetch(`${getActiveServer()}/api/admin/appeals/${appeal.id}`, {
				method: 'PATCH',
				headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${getToken()}` },
				body: JSON.stringify({ status, admin_notes: note || null })
			});
			if (!res.ok) {
				const data = await res.json().catch(() => ({}));
				throw new Error(data.error || 'Could not record the decision.');
			}
			appeals = appeals.map((a) =>
				a.id === appeal.id ? { ...a, status, admin_notes: note || null } : a
			);
		} catch (e) {
			setDecisionError(appeal.id, e instanceof Error ? e.message : 'Could not record the decision.');
		}
		deciding = null;
	}

	function when(iso: string): string {
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="container">
	<h1>Admin Dashboard</h1>

	<nav class="admin-nav">
		<a href="/admin/users">Users</a>
	</nav>

	{#if loading}
		<p class="status">Loading...</p>
	{:else if stats}
		<div class="stats-grid">
			<div class="stat"><span class="val">{stats.users}</span><span class="label">Users</span></div>
			<div class="stat"><span class="val">{stats.active_posts}</span><span class="label">Active Posts</span></div>
			<div class="stat"><span class="val">{stats.total_posts}</span><span class="label">Total Posts</span></div>
			<div class="stat"><span class="val">{stats.matches}</span><span class="label">Matches</span></div>
			<div class="stat"><span class="val">{stats.messages}</span><span class="label">Messages</span></div>
			<div class="stat"><span class="val">{stats.directory_entries}</span><span class="label">Directory</span></div>
		</div>
	{/if}

	<section class="appeals" aria-labelledby="appeals-heading">
		<h2 id="appeals-heading">Appeals</h2>
		{#if appealsState === 'loading'}
			<p class="hint">Loading appeals…</p>
		{:else if appealsState === 'error'}
			<p class="error" role="alert">Could not load appeals. Reload the page to try again.</p>
		{:else if waiting.length === 0}
			<p class="hint">No appeals are waiting for a decision.</p>
		{:else}
			<ul class="appeal-list">
				{#each waiting as appeal (appeal.id)}
					<li class="appeal">
						<h3 class="appeal-title"><a href="/p/{appeal.post_id}">{appeal.post_title}</a></h3>
						<p class="appeal-meta">From {appeal.author_name} · {when(appeal.created_at)}</p>
						<p class="appeal-text">
							<span class="appeal-label">Reason given</span>
							{appeal.reason}
						</p>
						<p class="appeal-text">
							<span class="appeal-label">Appeal</span>
							{appeal.body}
						</p>
						<label class="appeal-label" for="note-{appeal.id}">Note to the author</label>
						<textarea id="note-{appeal.id}" rows="2" bind:value={notes[appeal.id]}></textarea>
						{#if decisionErrors[appeal.id]}
							<p class="error" role="alert">{decisionErrors[appeal.id]}</p>
						{/if}
						<div class="appeal-actions">
							<button
								type="button"
								class="btn-primary"
								onclick={() => decide(appeal, 'granted')}
								disabled={deciding === appeal.id}
								aria-busy={deciding === appeal.id}
							>
								Restore post
							</button>
							<button
								type="button"
								class="btn-ghost decline-btn"
								onclick={() => decide(appeal, 'denied')}
								disabled={deciding === appeal.id}
								aria-busy={deciding === appeal.id}
							>
								Decline appeal
							</button>
						</div>
					</li>
				{/each}
			</ul>
		{/if}

		{#if decided.length > 0}
			<h3 class="decided-heading">Decided</h3>
			<ul class="appeal-list">
				{#each decided as appeal (appeal.id)}
					<li class="appeal">
						<p class="appeal-title">{appeal.post_title}</p>
						<p class="appeal-outcome">{DECIDED_LABELS[appeal.status] ?? 'Decided'}</p>
						{#if appeal.admin_notes}
							<p class="appeal-text">
								<span class="appeal-label">Note to the author</span>
								{appeal.admin_notes}
							</p>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</section>
</div>

<style>
	h1 { font-size: 1.5rem; margin-bottom: 1rem; }

	.admin-nav {
		display: flex;
		gap: 1rem;
		margin-bottom: 2rem;
		padding-bottom: 1rem;
		border-bottom: 1px solid var(--border);
	}

	.admin-nav a {
		color: var(--text-muted);
		padding: 0.4rem 0.8rem;
		border-radius: var(--radius);
		background: var(--bg-surface);
		border: 1px solid var(--border);
		font-size: 0.9rem;
	}

	.admin-nav a:hover { border-color: var(--accent); text-decoration: none; color: var(--text); }

	.stats-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 1rem;
	}

	.stat {
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 1.5rem;
		text-align: center;
	}

	.val { display: block; font-size: 2rem; font-weight: 700; }
	.label { display: block; font-size: 0.8rem; color: var(--text-muted); margin-top: 0.25rem; }

	.status { text-align: center; color: var(--text-muted); padding: 3rem 0; }

	.appeals { margin-top: var(--space-6); }
	.appeals h2 { font-size: var(--text-xl); margin-bottom: var(--space-3); }
	.appeal-list { list-style: none; display: flex; flex-direction: column; gap: var(--space-3); }
	.appeal {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: var(--space-4);
	}
	.appeal-title { font-size: var(--text-base); font-weight: 600; }
	.appeal-meta { color: var(--text-muted); font-size: var(--text-xs); }
	.appeal-text { font-size: var(--text-sm); white-space: pre-wrap; }
	.appeal-label { display: block; color: var(--text-muted); font-size: var(--text-xs); font-weight: 600; }
	.appeal-outcome { font-size: var(--text-sm); font-weight: 600; }
	.appeal-actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
	.decline-btn:disabled { color: var(--text-muted); cursor: not-allowed; }
	.decided-heading { font-size: var(--text-base); margin: var(--space-5) 0 var(--space-3); }
	.hint { color: var(--text-muted); font-size: var(--text-sm); }
	.error { color: var(--critical); font-size: var(--text-sm); }

	@media (max-width: 480px) {
		.stats-grid { grid-template-columns: repeat(2, 1fr); }
		.admin-nav { flex-wrap: wrap; }
	}
</style>
