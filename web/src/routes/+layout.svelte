<script lang="ts">
	import '../app.css';
	import '$lib/design/tokens.css';
	import { onMount } from 'svelte';
	import SearchBar from '$lib/components/SearchBar.svelte';
	import { auth, isAuthenticated, getToken, refreshRole, initAuth } from '$lib/stores/auth';
	import { serverState, getActiveServer } from '$lib/stores/server';
	import { initTheme } from '$lib/stores/theme';

	let { children } = $props();
	let unreadCount = $state(0);
	let menuOpen = $state(false);
	let online = $state(true);
	let offlineDismissed = $state(false);
	let installPrompt: any = $state(null);
	let showInstall = $state(false);

	onMount(() => {
		online = navigator.onLine;
		window.addEventListener('online', () => { online = true; offlineDismissed = false; });
		window.addEventListener('offline', () => { online = false; offlineDismissed = false; });

		initTheme();
		initAuth();

		if (!window.matchMedia('(display-mode: standalone)').matches) {
			window.addEventListener('beforeinstallprompt', (e: Event) => {
				e.preventDefault();
				installPrompt = e;
				showInstall = true;
			});
		}

		refreshRole();
		pollNotifications();
		const interval = setInterval(pollNotifications, 15000);
		return () => clearInterval(interval);
	});

	function handleInstall() {
		installPrompt?.prompt();
		showInstall = false;
	}

	async function pollNotifications() {
		if (!isAuthenticated()) return;
		const server = getActiveServer();
		const token = getToken();
		if (!server || !token) return;
		try {
			const res = await fetch(`${server}/api/me/notifications/count`, {
				headers: { 'Authorization': `Bearer ${token}` }
			});
			if (res.ok) {
				const data = await res.json();
				unreadCount = data.unread || 0;
			}
		} catch {}
	}

	function closeMenu() {
		menuOpen = false;
	}
</script>

<svelte:head>
	<title>Komun</title>
	<meta name="description" content="Local Listings" />
	<meta property="og:title" content="Komun" />
	<meta property="og:description" content="Local Listings" />
	<meta property="og:type" content="website" />
	<meta property="og:image" content="/favicon.svg" />
	<meta property="og:image:width" content="512" />
	<meta property="og:image:height" content="512" />
	<meta name="twitter:card" content="summary" />
	<meta name="twitter:title" content="Komun" />
	<meta name="twitter:description" content="Local Listings" />
	<meta name="twitter:image" content="/favicon.svg" />
</svelte:head>

<header>
	<nav class="container">
		<a href="/" class="logo" onclick={closeMenu}>
			<svg class="logo-icon" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
				<circle cx="8" cy="12" r="5"/>
				<circle cx="16" cy="12" r="5"/>
				<circle cx="12" cy="8" r="5"/>
			</svg>
			<span class="logo-text">komun</span>
		</a>

		<SearchBar />

		<button class="hamburger" class:open={menuOpen} onclick={() => menuOpen = !menuOpen} aria-label="Menu">
			<span class="h-square"></span><span class="h-square"></span><span class="h-square"></span>
		</button>

		<div class="nav-links" class:show={menuOpen}>
			{#if $serverState.active}
				<a href="/market" onclick={closeMenu}>Market</a>
				<a href="/aid" onclick={closeMenu}>Aid</a>
				{#if $auth.servers?.[$serverState.active]}
					<a href="/messages" onclick={closeMenu}>Messages</a>
					<a href="/notifications" class="notif-link" onclick={closeMenu}>
						Alerts
						{#if unreadCount > 0}
							<span class="notif-badge">{unreadCount}</span>
						{/if}
					</a>
					{#if $auth.servers[$serverState.active].role === 'superadmin'}
						<a href="/admin" class="admin-link" onclick={closeMenu}>Admin</a>
					{/if}
				{/if}
			{/if}
			<a href="/connect" onclick={closeMenu}>Connect</a>
			{#if $serverState.active && !$auth.servers?.[$serverState.active]}
				<a href="/account/signup" class="join-link" onclick={closeMenu}>Join</a>
			{/if}
			{#if $serverState.active && $auth.servers?.[$serverState.active]}
				<a href="/account" class="identity" onclick={closeMenu}>{$auth.servers[$serverState.active].displayName}</a>
			{/if}
		</div>
	</nav>
</header>

{#if !online && !offlineDismissed}
	<div class="offline-banner">
		<span>You're offline. Showing cached data.</span>
		<button class="dismiss-btn" onclick={() => offlineDismissed = true}>&times;</button>
	</div>
{/if}

<main>
	{@render children()}
</main>

{#if showInstall}
	<div class="install-banner">
		<span>Install Komun for faster access</span>
		<button class="install-btn" onclick={handleInstall}>Install</button>
		<button class="dismiss-btn" onclick={() => showInstall = false}>&times;</button>
	</div>
{/if}

<style>
	header {
		border-bottom: 1px solid var(--accent);
		padding: 0.6rem 0;
		position: sticky;
		top: 0;
		background: var(--bg);
		backdrop-filter: blur(8px);
		z-index: 100;
	}

	nav {
		display: flex;
		align-items: center;
		justify-content: space-between;
		position: relative;
	}

	.logo {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-family: var(--font-display);
		font-size: 1.3rem;
		font-weight: 800;
		color: var(--text);
		letter-spacing: -0.5px;
		z-index: 2;
		text-decoration: none;
	}

	.logo:hover {
		color: var(--accent);
		text-decoration: none;
	}

	.logo-icon {
		flex-shrink: 0;
	}

	.hamburger {
		display: none;
		flex-direction: column;
		gap: 4px;
		background: none;
		padding: 8px;
		z-index: 2;
		min-height: 44px;
		min-width: 44px;
		align-items: center;
		justify-content: center;
	}

	.h-square {
		display: block;
		width: 5px;
		height: 5px;
		background: var(--text);
		border-radius: var(--radius-sm);
		transition: all var(--transition-base);
	}

	.hamburger.open .h-square:nth-child(1) {
		transform: translate(6px, 4px);
	}
	.hamburger.open .h-square:nth-child(2) {
		transform: scale(0);
	}
	.hamburger.open .h-square:nth-child(3) {
		transform: translate(-6px, -4px);
	}

	.nav-links {
		display: flex;
		gap: 0.35rem;
		align-items: center;
	}

	.nav-links a {
		color: var(--text);
		font-size: 0.85rem;
		padding: 0.35rem 0.7rem;
		border-radius: var(--radius-full);
		transition: background var(--transition-fast), color var(--transition-fast);
		opacity: 0.8;
	}

	.nav-links a:hover {
		background: var(--button-ghost-hover);
		color: var(--text);
		text-decoration: none;
		opacity: 1;
	}

	.notif-link {
		position: relative;
	}

	.notif-badge {
		position: absolute;
		top: -4px;
		right: -6px;
		background: var(--critical);
		color: var(--text-on-critical);
		font-size: 0.6rem;
		font-weight: 700;
		min-width: 18px;
		height: 18px;
		border-radius: var(--radius-full);
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0 5px;
		animation: badge-pulse 2s ease-in-out infinite;
	}

	@keyframes badge-pulse {
		0%, 100% { transform: scale(1); }
		50% { transform: scale(1.1); }
	}

	.admin-link {
		color: var(--warning) !important;
		font-weight: 600;
	}

	.join-link {
		/* An <a>: reset the inherited anchor defaults. */
		display: inline-block;
		text-decoration: none;
		background: var(--accent);
		color: var(--text-on-accent);
		font-size: 0.8rem;
		font-weight: 700;
		padding: 0.35rem 0.8rem;
		border-radius: var(--radius-full);
		transition: transform var(--transition-fast), opacity var(--transition-fast);
	}

	.join-link:hover {
		transform: translateY(-1px);
		opacity: 0.9;
	}

	.identity {
		color: var(--success) !important;
		font-size: 0.8rem;
		font-weight: 600;
		padding: 0.3rem 0.7rem;
		background: var(--success-softer);
		border-radius: var(--radius-full);
	}

	.identity:hover {
		text-decoration: none !important;
		background: var(--success-soft);
	}

	main {
		padding: var(--space-5) 0;
		animation: page-in var(--transition-slow) ease;
	}

	@keyframes page-in {
		from { opacity: 0; transform: translateY(4px); }
		to   { opacity: 1; transform: translateY(0); }
	}

	.offline-banner {
		background: var(--warning);
		color: var(--text-on-warning);
		padding: 0.5rem 1rem;
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 1rem;
		font-size: 0.85rem;
		font-weight: 600;
	}

	.install-banner {
		position: fixed;
		bottom: 0;
		left: 0;
		right: 0;
		background: var(--bg-surface);
		border-top: 1px solid var(--border);
		padding: 0.75rem 1rem;
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 1rem;
		font-size: 0.9rem;
		z-index: 200;
	}

	.install-btn {
		background: var(--accent);
		color: var(--text-on-accent);
		padding: 0.4rem 1rem;
		border-radius: var(--radius-full);
		font-weight: 600;
		font-size: 0.85rem;
	}

	.dismiss-btn {
		background: none;
		color: inherit;
		font-size: 1.2rem;
		opacity: 0.7;
		min-height: 30px;
		min-width: 30px;
	}

	@media (max-width: 768px) {
		.hamburger {
			display: flex;
		}

		.nav-links {
			display: none;
			position: fixed;
			top: 48px;
			left: 0;
			right: 0;
			flex-direction: column;
			background: var(--bg-surface);
			border-bottom: 1px solid var(--border);
			padding: var(--space-3);
			gap: 0;
			z-index: 100;
		}

		.nav-links.show {
			display: flex;
		}

		.nav-links a {
			padding: 0.75rem 0.7rem;
			font-size: var(--text-base);
			border-bottom: 1px solid var(--border);
			border-radius: 0;
			width: 100%;
		}

		.nav-links a:last-of-type {
			border-bottom: none;
		}

		.identity {
			margin-top: 0.75rem;
			text-align: center;
		}

		.notif-badge {
			position: static;
			display: inline-flex;
			margin-left: 0.5rem;
		}
	}
</style>
