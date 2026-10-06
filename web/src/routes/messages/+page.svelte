<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { isConnected } from '$lib/stores/server';
	import { isAuthenticated } from '$lib/stores/auth';
	import { api } from '$lib/api/client';

	interface Conversation {
		match_id: string;
		post_title: string;
		post_kind: string;
		other_party_name: string;
		last_message?: string;
		last_message_at?: string;
		status: string;
	}

	let conversations: Conversation[] = $state([]);
	let loading = $state(true);

	onMount(() => {
		if (!isConnected() || !isAuthenticated()) {
			goto('/');
			return;
		}
		loadConversations();
		const interval = setInterval(loadConversations, 15000);
		return () => clearInterval(interval);
	});

	async function loadConversations() {
		try {
			conversations = await api.conversations.list();
		} catch (e) {}
		loading = false;
	}

	function timeAgo(dateStr?: string): string {
		if (!dateStr) return '';
		const seconds = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
		return `${Math.floor(seconds / 86400)}d ago`;
	}
</script>

<div class="container">
	<h1>Messages</h1>

	{#if loading}
		<p class="status">Loading...</p>
	{:else if conversations.length === 0}
		<div class="empty">
			<p>No conversations yet.</p>
			<p class="sub">Respond to a post to start a conversation.</p>
		</div>
	{:else}
		<ul class="conversation-list">
			{#each conversations as convo}
				<li>
					<a href="/messages/{convo.match_id}">
						<div class="convo-header">
							<span class="kind-dot kind-{convo.post_kind}"></span>
							<strong>{convo.post_title}</strong>
							<span class="status-badge status-{convo.status}">{convo.status}</span>
						</div>
						<div class="convo-preview">
							<span class="other-name">{convo.other_party_name}:</span>
							<span class="last-msg">{convo.last_message || 'No messages yet'}</span>
						</div>
						<span class="time">{timeAgo(convo.last_message_at)}</span>
					</a>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	h1 { font-size: 1.5rem; margin-bottom: 1.5rem; }

	.conversation-list {
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	li {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		overflow: hidden;
	}

	li a {
		display: block;
		padding: 1rem;
		color: var(--text);
	}

	li a:hover {
		background: var(--bg-surface);
		text-decoration: none;
	}

	.convo-header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin-bottom: 0.4rem;
	}

	.kind-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
	}

	.kind-need { background: var(--critical); }
	.kind-offer { background: var(--success); }
	.kind-resource { background: var(--kind-resource); }

	.status-badge {
		margin-left: auto;
		font-size: 0.7rem;
		padding: 0.1rem 0.4rem;
		border-radius: var(--radius-sm);
		text-transform: uppercase;
		font-weight: 600;
	}

	.status-proposed { background: var(--warning-soft); color: var(--warning); }
	.status-accepted { background: var(--success-soft); color: var(--success); }
	.status-completed { background: var(--success-strong); color: var(--success); }

	.convo-preview {
		font-size: 0.85rem;
		color: var(--text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.other-name { font-weight: 500; color: var(--text); }
	.last-msg { color: var(--text-muted); }

	.time {
		font-size: 0.75rem;
		color: var(--text-muted);
		margin-top: 0.25rem;
		display: block;
	}

	.status { text-align: center; color: var(--text-muted); padding: 3rem 0; }
	.empty { text-align: center; padding: 3rem 0; color: var(--text-muted); }
	.empty .sub { font-size: 0.85rem; margin-top: 0.5rem; }
</style>
