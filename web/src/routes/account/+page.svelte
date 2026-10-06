<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import {
		auth,
		getActiveAuth,
		getEncryptionPublicKey,
		getEncryptionSecretKey,
		getToken,
		isAuthenticated,
		changePassword,
		reissueRecoveryCode,
		listSessions,
		revokeSession,
		revokeOtherSessions,
		resendVerification,
		updateDisplayName,
		logout,
		type SessionSummary,
	} from '$lib/stores/auth';
	import { isConnected, getActiveServer, serverState, removeServer } from '$lib/stores/server';
	import { themeName } from '$lib/stores/theme';
	import ThemePicker from '$lib/components/ThemePicker.svelte';
	import RecoveryCode from './RecoveryCode.svelte';

	const MIN_PASSWORD = 12;

	let email = $state('');
	let emailVerified = $state(true);
	let displayName = $state('');
	let bio = $state('');
	let avatarUrl = $state('');
	let saving = $state(false);
	let saved = $state(false);
	let bioSaving = $state(false);
	let bioSaved = $state(false);
	let avatarUploading = $state(false);
	let error = $state('');
	let bioError = $state('');
	let copied = $state(false);
	let showTheme = $state(false);
	let resendState = $state('');

	let currentPassword = $state('');
	let newPassword = $state('');
	let newPasswordConfirm = $state('');
	let pwSaving = $state(false);
	let pwSaved = $state(false);
	let pwError = $state('');

	let reissuePassword = $state('');
	let reissuing = $state(false);
	let reissueError = $state('');
	let freshRecoveryCode = $state('');

	let sessions = $state<SessionSummary[]>([]);
	let sessionsError = $state('');

	const publicKey = $derived(getEncryptionPublicKey() || '');
	const keyUnlocked = $derived(!!$auth.keypair?.secretKey);

	onMount(async () => {
		if (!isConnected() || !isAuthenticated()) {
			goto('/account/login');
			return;
		}
		const server = getActiveServer();
		const token = getToken();
		if (server && token) {
			try {
				const res = await fetch(`${server}/api/auth/me`, {
					headers: { Authorization: `Bearer ${token}` },
				});
				if (res.ok) {
					const data = await res.json();
					displayName = data.display_name || '';
					bio = data.bio || '';
					avatarUrl = data.avatar_url || '';
					email = data.email || getActiveAuth()?.email || '';
					emailVerified = data.email_verified === true;
				}
			} catch {
				// leave the form blank rather than blocking the page
			}
		}
		await loadSessions();
	});

	async function loadSessions() {
		sessionsError = '';
		try {
			sessions = await listSessions();
		} catch {
			sessionsError = 'Could not load your sessions.';
		}
	}

	async function handleResend() {
		resendState = 'sending';
		const result = await resendVerification(email);
		resendState = result.ok ? 'sent' : 'failed';
	}

	async function saveName() {
		if (!displayName.trim()) { error = 'Name is required'; return; }
		saving = true;
		error = '';
		const ok = await updateDisplayName(displayName.trim());
		saving = false;
		if (ok) { saved = true; setTimeout(() => (saved = false), 2000); }
		else { error = 'Failed to update name'; }
	}

	async function saveBio() {
		bioSaving = true;
		bioError = '';
		try {
			const res = await fetch(`${getActiveServer()}/api/auth/me`, {
				method: 'PUT',
				headers: {
					'Content-Type': 'application/json',
					Authorization: `Bearer ${getToken()}`,
				},
				body: JSON.stringify({ bio: bio.trim() || null }),
			});
			bioSaving = false;
			if (res.ok) { bioSaved = true; setTimeout(() => (bioSaved = false), 2000); }
			else { bioError = 'Failed to save bio'; }
		} catch {
			bioSaving = false;
			bioError = 'Network error';
		}
	}

	async function handleAvatarUpload(e: Event) {
		const file = (e.target as HTMLInputElement).files?.[0];
		if (!file) return;
		avatarUploading = true;
		try {
			const formData = new FormData();
			formData.append('file', file);
			const res = await fetch(`${getActiveServer()}/api/auth/me/avatar`, {
				method: 'POST',
				headers: { Authorization: `Bearer ${getToken()}` },
				body: formData,
			});
			if (res.ok) {
				const data = await res.json();
				avatarUrl = data.avatar_url;
			}
		} catch {
			// the preview simply does not update
		}
		avatarUploading = false;
	}

	async function handleChangePassword() {
		pwError = '';
		pwSaved = false;
		if (newPassword.length < MIN_PASSWORD) {
			pwError = `New password must be at least ${MIN_PASSWORD} characters.`;
			return;
		}
		if (newPassword !== newPasswordConfirm) {
			pwError = 'New passwords do not match.';
			return;
		}
		if (!getEncryptionSecretKey()) {
			pwError =
				'Your encryption key is locked on this device, so it cannot be re-wrapped under a new password. Sign in again first.';
			return;
		}

		pwSaving = true;
		const result = await changePassword(currentPassword, newPassword);
		pwSaving = false;

		currentPassword = '';
		newPassword = '';
		newPasswordConfirm = '';

		if (!result.ok) {
			pwError = result.error || 'Could not change the password.';
			return;
		}
		pwSaved = true;
		// Every other session was revoked server-side; the list on screen is now wrong.
		await loadSessions();
	}

	async function handleReissue() {
		reissueError = '';
		reissuing = true;
		const result = await reissueRecoveryCode(reissuePassword);
		reissuing = false;
		reissuePassword = '';
		if (!result.ok || !result.recoveryCode) {
			reissueError = result.error || 'Could not issue a new code.';
			return;
		}
		freshRecoveryCode = result.recoveryCode;
	}

	async function handleRevoke(id: string) {
		if (await revokeSession(id)) await loadSessions();
	}

	async function handleRevokeOthers() {
		await revokeOtherSessions();
		await loadSessions();
	}

	function copyPublicKey() {
		navigator.clipboard.writeText(publicKey);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	function handleLogout() {
		logout();
		goto('/');
	}

	function when(iso: string): string {
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<svelte:head><title>Account · Komun</title></svelte:head>

<div class="container">
	<h1>Account</h1>

	{#if !emailVerified}
		<div class="banner">
			<div>
				<strong>Your email is not verified yet.</strong>
				<p>
					Until it is, you can read and post but you will not get reset links — which means
					a forgotten password would lock you out for good.
				</p>
			</div>
			<button class="banner-btn" onclick={handleResend} disabled={resendState === 'sending'}>
				{resendState === 'sent'
					? 'Link sent'
					: resendState === 'sending'
						? 'Sending…'
						: resendState === 'failed'
							? 'Try again'
							: 'Resend link'}
			</button>
		</div>
	{/if}

	<section class="section">
		<h2>Profile Picture</h2>
		<div class="avatar-section">
			{#if avatarUrl}
				<img src={avatarUrl} alt="Your avatar" class="avatar-preview" />
			{:else}
				<div class="avatar-placeholder">{displayName[0]?.toUpperCase() || '?'}</div>
			{/if}
			<label class="upload-label">
				{avatarUploading ? 'Uploading...' : 'Upload photo'}
				<input type="file" accept="image/png,image/jpeg,image/webp" onchange={handleAvatarUpload} disabled={avatarUploading} class="file-input" />
			</label>
			<p class="hint">PNG, JPEG, or WebP. Max 1MB. EXIF data is stripped.</p>
		</div>
	</section>

	<section class="section">
		<h2>Display Name</h2>
		<form onsubmit={(e) => { e.preventDefault(); saveName(); }}>
			<input type="text" bind:value={displayName} maxlength="50" />
			{#if error}<p class="error">{error}</p>{/if}
			<button type="submit" class="save-btn" disabled={saving}>
				{saving ? 'Saving...' : saved ? 'Saved!' : 'Update Name'}
			</button>
		</form>
	</section>

	<section class="section">
		<h2>Bio</h2>
		<form onsubmit={(e) => { e.preventDefault(); saveBio(); }}>
			<textarea bind:value={bio} maxlength="500" placeholder="Tell people about yourself..." rows="4"></textarea>
			{#if bioError}<p class="error">{bioError}</p>{/if}
			<button type="submit" class="save-btn" disabled={bioSaving}>
				{bioSaving ? 'Saving...' : bioSaved ? 'Saved!' : 'Save Bio'}
			</button>
		</form>
	</section>

	<section class="section">
		<h2>Encryption key</h2>
		<p class="hint">
			Other people encrypt messages to this key. The matching secret half never leaves your
			device, and is unlocked automatically when you sign in.
		</p>
		{#if publicKey}
			<div class="key-display">
				<code>{publicKey.slice(0, 20)}...{publicKey.slice(-8)}</code>
				<button class="copy-btn" onclick={copyPublicKey}>{copied ? 'Copied!' : 'Copy'}</button>
			</div>
		{:else}
			<p class="hint">No encryption key on this account.</p>
		{/if}
		{#if !keyUnlocked}
			<p class="warn-line">
				Locked on this device — sign in again to read encrypted messages here.
			</p>
		{/if}
	</section>

	<section class="section">
		<h2>Change password</h2>
		<p class="hint">
			Your encryption key is re-wrapped under the new password, so nothing you could read
			before becomes unreadable. Your recovery code keeps working. Other devices are signed out.
		</p>
		<form onsubmit={(e) => { e.preventDefault(); handleChangePassword(); }}>
			<input type="password" autocomplete="current-password" bind:value={currentPassword} placeholder="Current password" />
			<input type="password" autocomplete="new-password" bind:value={newPassword} placeholder="New password" />
			<input type="password" autocomplete="new-password" bind:value={newPasswordConfirm} placeholder="Confirm new password" />
			{#if pwError}<p class="error">{pwError}</p>{/if}
			{#if pwSaved}<p class="success">Password changed. Other sessions were signed out.</p>{/if}
			<button type="submit" class="save-btn" disabled={pwSaving}>
				{pwSaving ? 'Changing…' : 'Change password'}
			</button>
		</form>
	</section>

	<section class="section">
		<h2>Recovery code</h2>
		{#if freshRecoveryCode}
			<RecoveryCode
				code={freshRecoveryCode}
				{email}
				doneLabel="Done"
				onDone={() => (freshRecoveryCode = '')}
			/>
		{:else}
			<p class="hint">
				Issuing a new code invalidates the old one immediately. We cannot show you the
				current code — the server has never held it, only your key sealed under it.
			</p>
			<form onsubmit={(e) => { e.preventDefault(); handleReissue(); }}>
				<input type="password" autocomplete="current-password" bind:value={reissuePassword} placeholder="Current password" />
				{#if reissueError}<p class="error">{reissueError}</p>{/if}
				<button type="submit" class="save-btn" disabled={reissuing}>
					{reissuing ? 'Issuing…' : 'Issue a new recovery code'}
				</button>
			</form>
		{/if}
	</section>

	<section class="section">
		<h2>Signed-in devices</h2>
		{#if sessionsError}
			<p class="error">{sessionsError}</p>
		{:else if sessions.length === 0}
			<p class="hint">No active sessions.</p>
		{:else}
			<ul class="session-list">
				{#each sessions as s (s.id)}
					<li>
						<div>
							<strong>{s.device_label || 'Unnamed device'}</strong>
							{#if s.current}<span class="active-badge">this device</span>{/if}
							<span class="session-meta">
								{s.ip || 'unknown address'} · last used {when(s.last_used_at)}
							</span>
						</div>
						{#if !s.current}
							<button class="remove-btn" onclick={() => handleRevoke(s.id)}>Sign out</button>
						{/if}
					</li>
				{/each}
			</ul>
			{#if sessions.length > 1}
				<button class="remove-btn wide" onclick={handleRevokeOthers}>
					Sign out every other device
				</button>
			{/if}
		{/if}
	</section>

	<section class="section">
		<h2>Theme</h2>
		<div class="theme-pick-row">
			<span class="current-theme">{$themeName}</span>
			<button class="change-btn" onclick={() => (showTheme = true)}>Change</button>
		</div>
	</section>

	<section class="section">
		<h2>Connected Servers</h2>
		{#if $serverState.known.length === 0}
			<p class="hint">No servers connected.</p>
		{:else}
			<ul class="server-list">
				{#each $serverState.known as server}
					<li>
						<div>
							<strong>{server.name}</strong>
							<span class="server-url">{server.url}</span>
							{#if server.url === $serverState.active}
								<span class="active-badge">active</span>
							{/if}
						</div>
						<button class="remove-btn" onclick={() => removeServer(server.url)}>Remove</button>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section class="section danger-zone">
		<button class="logout-btn" onclick={handleLogout}>Log out of this server</button>
	</section>
</div>

<ThemePicker shown={showTheme} onClose={() => (showTheme = false)} />

<style>
	h1 { font-size: 1.5rem; margin-bottom: 1.5rem; }
	h2 { font-size: 1.1rem; margin-bottom: 0.5rem; }

	.section {
		margin-bottom: 2rem;
		padding-bottom: 2rem;
		border-bottom: 1px solid var(--border);
	}

	.hint { color: var(--text-muted); font-size: 0.85rem; margin-bottom: 0.75rem; max-width: 440px; }
	.warn-line { color: var(--critical); font-size: 0.85rem; margin-top: 0.5rem; }

	.banner {
		display: flex;
		gap: 1rem;
		align-items: center;
		justify-content: space-between;
		background: var(--critical-soft);
		border: 1px solid var(--critical);
		border-radius: var(--radius);
		padding: 0.8rem 1rem;
		margin-bottom: 1.5rem;
	}
	.banner p { font-size: 0.85rem; color: var(--text-muted); margin-top: 0.2rem; }
	.banner-btn {
		background: var(--bg-elevated);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.45rem 0.8rem;
		font-size: 0.85rem;
		white-space: nowrap;
	}

	form { display: flex; flex-direction: column; gap: 0.6rem; max-width: 400px; }
	input, textarea { background: var(--bg-surface); border: 1px solid var(--border); border-radius: var(--radius); padding: 0.75rem; color: var(--text); font-size: 1rem; }
	input:focus, textarea:focus { outline: none; border-color: var(--accent); }

	.save-btn { background: var(--accent); color: var(--text-on-accent); padding: 0.6rem; border-radius: var(--radius); font-weight: 600; }
	.save-btn:disabled { opacity: 0.6; }

	.avatar-section {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.75rem;
	}

	.avatar-preview {
		width: 96px;
		height: 96px;
		border-radius: 50%;
		object-fit: cover;
		border: 2px solid var(--border);
	}

	.avatar-placeholder {
		width: 96px;
		height: 96px;
		border-radius: 50%;
		background: var(--bg-elevated);
		border: 2px solid var(--border);
		display: flex;
		align-items: center;
		justify-content: center;
		font-size: 2.5rem;
		font-weight: 700;
		color: var(--text-muted);
	}

	.upload-label {
		background: var(--bg-elevated);
		color: var(--text);
		padding: 0.4rem 0.8rem;
		border-radius: var(--radius);
		font-size: 0.85rem;
		border: 1px solid var(--border);
		cursor: pointer;
	}

	.file-input { display: none; }

	.key-display {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 0.6rem 0.8rem;
		max-width: 400px;
	}

	code { font-size: 0.85rem; color: var(--text-muted); flex: 1; word-break: break-all; }
	.copy-btn { background: var(--bg-elevated); color: var(--text); padding: 0.3rem 0.6rem; border-radius: var(--radius); font-size: 0.8rem; border: 1px solid var(--border); }

	.session-list, .server-list { list-style: none; max-width: 500px; }
	.session-list li, .server-list li {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 0.75rem;
		padding: 0.6rem 0.8rem;
		background: var(--bg-surface);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		margin-bottom: 0.4rem;
	}

	.session-meta { display: block; color: var(--text-muted); font-size: 0.75rem; }
	.server-url { display: block; color: var(--text-muted); font-size: 0.75rem; }
	.active-badge { font-size: 0.65rem; color: var(--success); background: var(--success-softer); padding: 0.1rem 0.4rem; border-radius: var(--radius-sm); margin-left: 0.4rem; }
	.remove-btn { background: none; color: var(--critical); font-size: 0.8rem; padding: 0.2rem 0.5rem; border: 1px solid var(--critical); border-radius: var(--radius); white-space: nowrap; }
	.remove-btn.wide { margin-top: 0.5rem; padding: 0.4rem 0.8rem; }

	.error { color: var(--critical); font-size: 0.85rem; }
	.success { color: var(--success); font-weight: 600; font-size: 0.9rem; }

	.danger-zone { border-bottom: none; }
	.logout-btn { background: none; color: var(--critical); padding: 0.6rem 1rem; border: 1px solid var(--critical); border-radius: var(--radius); font-weight: 600; }
	.logout-btn:hover { background: var(--critical-soft); }

	.theme-pick-row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.current-theme {
		color: var(--text);
		font-weight: 500;
		font-size: 0.95rem;
	}

	.change-btn {
		background: var(--bg-elevated);
		color: var(--text);
		padding: 0.35rem 0.75rem;
		border-radius: var(--radius);
		font-size: 0.8rem;
		border: 1px solid var(--border);
	}

	.change-btn:hover {
		border-color: var(--accent);
	}
</style>
