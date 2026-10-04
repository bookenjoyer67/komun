<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { isSuperadmin } from '$lib/stores/auth';
	import { isConnected } from '$lib/stores/server';
	import { api } from '$lib/api/client';

	let stats: any = $state(null);
	let loading = $state(true);

	onMount(() => {
		if (!isConnected() || !isSuperadmin()) { goto('/'); return; }
		loadStats();
	});

	async function loadStats() {
		try { stats = await api.admin.stats(); } catch {}
		loading = false;
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

	@media (max-width: 480px) {
		.stats-grid { grid-template-columns: repeat(2, 1fr); }
		.admin-nav { flex-wrap: wrap; }
	}
</style>
