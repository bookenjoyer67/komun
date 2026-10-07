<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { isConnected, getActiveServer } from '$lib/stores/server';
	import { isAuthenticated, getToken } from '$lib/stores/auth';
	import { safeAppPath } from '$lib/safeUrl';

	interface Notification {
		id: string;
		kind: string;
		title: string;
		body?: string;
		link?: string;
		read: boolean;
		created_at: string;
	}

	let notifications: Notification[] = $state([]);
	let loading = $state(true);

	onMount(() => {
		if (!isConnected() || !isAuthenticated()) { goto('/'); return; }
		loadNotifications();
	});

	async function loadNotifications() {
		const server = getActiveServer();
		const token = getToken();
		if (!server || !token) return;
		try {
			const res = await fetch(`${server}/api/me/notifications`, {
				headers: { 'Authorization': `Bearer ${token}` }
			});
			if (res.ok) notifications = await res.json();
		} catch {}
		loading = false;
	}

	async function markAllRead() {
		const server = getActiveServer();
		const token = getToken();
		if (!server || !token) return;
		await fetch(`${server}/api/me/notifications/read-all`, {
			method: 'POST',
			headers: { 'Authorization': `Bearer ${token}` }
		});
		notifications = notifications.map((n) => ({ ...n, read: true }));
	}

	async function handleClick(notif: Notification) {
		if (!notif.read) {
			const server = getActiveServer();
			const token = getToken();
			if (server && token) {
				fetch(`${server}/api/me/notifications/${notif.id}/read`, {
					method: 'PATCH',
					headers: { 'Authorization': `Bearer ${token}` }
				});
			}
		}
		// The SPA's origin, not the active server's: the link is a route in this app, and the API
		// server may live elsewhere.
		const path = safeAppPath(notif.link, window.location.origin);
		if (path) goto(path);
	}

	function timeAgo(dateStr: string): string {
		const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
		return `${Math.floor(seconds / 86400)}d ago`;
	}
</script>

<div class="container">
	<header class="page-header">
		<h1>Notifications</h1>
		{#if notifications.some((n) => !n.read)}
			<button class="mark-all" onclick={markAllRead}>Mark all read</button>
		{/if}
	</header>

	{#if loading}
		<p class="status">Loading...</p>
	{:else if notifications.length === 0}
		<p class="status">No notifications yet.</p>
	{:else}
		<ul class="notif-list">
			{#each notifications as notif}
				<li class:unread={!notif.read}>
					<button class="notif-item" onclick={() => handleClick(notif)}>
						<div class="notif-content">
							<strong>{notif.title}</strong>
							{#if notif.body}
								<p>{notif.body}</p>
							{/if}
						</div>
						<span class="time">{timeAgo(notif.created_at)}</span>
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.page-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 1.5rem;
	}

	h1 { font-size: 1.5rem; }

	.mark-all {
		background: none;
		color: var(--accent);
		font-size: 0.85rem;
	}

	.notif-list { list-style: none; }

	li {
		border-bottom: 1px solid var(--border);
	}

	li.unread {
		background: var(--bg-surface);
		border-left: 3px solid var(--accent);
	}

	.notif-item {
		width: 100%;
		text-align: left;
		background: none;
		color: var(--text);
		padding: 0.75rem 1rem;
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		gap: 1rem;
	}

	.notif-item:hover { background: var(--bg-elevated); }

	.notif-content { flex: 1; }
	.notif-content strong { font-size: 0.9rem; }
	.notif-content p { color: var(--text-muted); font-size: 0.8rem; margin-top: 0.2rem; }

	.time { color: var(--text-muted); font-size: 0.75rem; white-space: nowrap; }

	.status { text-align: center; color: var(--text-muted); padding: 3rem 0; }
</style>
