import { writable, get } from 'svelte/store';
import { goto } from '$app/navigation';
import { getActiveServer } from './server';
import {
	generateIdentityKeypair,
	generateSaltB64,
	deriveVerifier,
	wrapSecret,
	unwrapSecret,
	generateRecoveryCode,
	normalizeRecoveryCode,
	type IdentityKeypair,
} from '$lib/crypto';

/**
 * Auth state.
 *
 * `localStorage` keeps only non-secret session state (token, user id, display name, role): the
 * server stores a hash of the token and revocation takes effect on the next request. The x25519
 * secret is unwrapped at sign-in, held in memory and mirrored to `sessionStorage` so a reload in
 * the same tab keeps decrypting; never `localStorage`, and dropped on sign-out.
 */
interface PerServerAuth {
	token: string;
	userId: string;
	displayName: string;
	role: string;
	email?: string;
	emailVerified?: boolean;
}

interface AuthState {
	keypair: IdentityKeypair | null;
	servers: Record<string, PerServerAuth>;
}

const STORAGE_KEY = 'komun_auth';
const SESSION_KEY = 'komun_auth_session';

export const auth = writable<AuthState>({ keypair: null, servers: {} });

function emptyState(): AuthState {
	return { keypair: null, servers: {} };
}

function loadFromStorage(): AuthState {
	if (typeof localStorage === 'undefined') return emptyState();

	let servers: Record<string, PerServerAuth> = {};
	const raw = localStorage.getItem(STORAGE_KEY);
	if (raw) {
		try {
			const parsed = JSON.parse(raw);
			if (parsed && typeof parsed.servers === 'object' && parsed.servers) {
				servers = parsed.servers;
			}
		} catch {
			// A corrupt blob is not worth a crash; the user signs in again.
		}
	}

	let keypair: IdentityKeypair | null = null;
	const sessionRaw = sessionStorage.getItem(SESSION_KEY);
	if (sessionRaw) {
		try {
			const parsed = JSON.parse(sessionRaw);
			if (parsed?.publicKey && parsed?.secretKey) keypair = parsed;
		} catch {
		}
	}

	return { keypair, servers };
}

function saveToStorage(state: AuthState) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, JSON.stringify({ servers: state.servers }));
	if (state.keypair) {
		sessionStorage.setItem(SESSION_KEY, JSON.stringify(state.keypair));
	} else {
		sessionStorage.removeItem(SESSION_KEY);
	}
}

let _initPromise: Promise<void> | null = null;

export async function initAuth(): Promise<void> {
	if (_initPromise) return _initPromise;
	_initPromise = Promise.resolve().then(() => {
		auth.set(loadFromStorage());
		auth.subscribe((s) => saveToStorage(s));
	});
	return _initPromise;
}

/** Forget the in-memory key without touching the session. Used when locking a shared device. */
export function lockAuth() {
	auth.update((s) => ({ ...s, keypair: null }));
	if (typeof sessionStorage !== 'undefined') sessionStorage.removeItem(SESSION_KEY);
}

export function getActiveAuth(): PerServerAuth | null {
	const server = getActiveServer();
	if (!server) return null;
	return get(auth).servers[server] || null;
}

export function isAuthenticated(): boolean {
	return getActiveAuth() !== null;
}

export function getToken(): string | null {
	return getActiveAuth()?.token || null;
}

export function getDisplayName(): string | null {
	return getActiveAuth()?.displayName || null;
}

export function getEncryptionSecretKey(): string | null {
	return get(auth).keypair?.secretKey || null;
}

export function getEncryptionPublicKey(): string | null {
	return get(auth).keypair?.publicKey || null;
}

export function isSuperadmin(): boolean {
	return getActiveAuth()?.role === 'superadmin';
}

export function isEmailVerified(): boolean {
	return getActiveAuth()?.emailVerified === true;
}

export interface AuthResult {
	ok: boolean;
	error?: string;
	/** Present exactly once, on the calls that mint one. Never returned by the server. */
	recoveryCode?: string;
}

async function errorFrom(res: Response): Promise<string> {
	try {
		const data = await res.json();
		if (typeof data?.error === 'string') return data.error;
		if (typeof data?.message === 'string') return data.message;
	} catch {
	}
	if (res.status === 429) return 'Too many attempts. Wait a minute and try again.';
	return `Request failed (${res.status})`;
}

function storeSession(server: string, data: Record<string, unknown>, email?: string) {
	auth.update((s) => ({
		...s,
		servers: {
			...s.servers,
			[server]: {
				token: data.token as string,
				userId: data.user_id as string,
				displayName: data.display_name as string,
				role: (data.role as string) || 'user',
				email,
				emailVerified: data.email_verified === true,
			},
		},
	}));
}

function setKeypair(keypair: IdentityKeypair | null) {
	auth.update((s) => ({ ...s, keypair }));
}

/** The x25519 public key, read back from the account's own key endpoint (sign-in returns only the wrapped secret). */
async function fetchOwnPublicKey(server: string, userId: string, token: string): Promise<string> {
	const res = await fetch(`${server}/api/auth/users/${userId}/keys`, {
		headers: { Authorization: `Bearer ${token}` },
	});
	if (!res.ok) return '';
	const data = await res.json();
	return data.encryption_public_key || '';
}

/** The public salt an account's verifier is derived under. */
async function fetchAuthSalt(server: string, email: string): Promise<string | null> {
	const res = await fetch(`${server}/api/auth/salt?email=${encodeURIComponent(email)}`);
	if (!res.ok) return null;
	const data = await res.json();
	return data.auth_salt || null;
}

export interface SignupInput {
	email: string;
	displayName: string;
	password: string;
	inviteCode?: string;
}

/**
 * Create an account. The password and recovery code never leave the browser; the wire carries the
 * Argon2id verifier plus two ciphertexts wrapping one x25519 secret. The recovery code is returned
 * to show once and is never stored.
 */
export async function signup(input: SignupInput): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };

	try {
		const keypair = await generateIdentityKeypair();
		const authSalt = await generateSaltB64();
		const verifier = await deriveVerifier(input.password, authSalt);

		const wrapped = await wrapSecret(keypair.secretKey, input.password);
		const recoveryCode = await generateRecoveryCode();
		const recoveryWrapped = await wrapSecret(keypair.secretKey, recoveryCode);

		const res = await fetch(`${server}/api/auth/signup`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				email: input.email.trim(),
				display_name: input.displayName.trim(),
				verifier,
				auth_salt: authSalt,
				password_length: input.password.length,
				encryption_public_key: keypair.publicKey,
				encrypted_key_bundle: wrapped.bundle,
				bundle_salt: wrapped.salt,
				encrypted_recovery_bundle: recoveryWrapped.bundle,
				recovery_bundle_salt: recoveryWrapped.salt,
				invite_code: input.inviteCode || null,
			}),
		});

		if (!res.ok) return { ok: false, error: await errorFrom(res) };

		const data = await res.json();
		setKeypair(keypair);
		storeSession(server, data, input.email.trim());
		return { ok: true, recoveryCode };
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Signup failed' };
	}
}

/**
 * Sign in and unlock the encryption key in the same step: the password is already in hand, so the
 * unwrap costs one extra Argon2id pass. A bundle that fails to open is reported, not swallowed.
 */
export async function login(
	email: string,
	password: string,
	deviceLabel?: string
): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };

	try {
		const authSalt = await fetchAuthSalt(server, email.trim());
		if (!authSalt) return { ok: false, error: 'Could not start sign-in. Try again.' };

		const verifier = await deriveVerifier(password, authSalt);

		const res = await fetch(`${server}/api/auth/signin`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				email: email.trim(),
				verifier,
				device_label: deviceLabel || null,
			}),
		});

		if (!res.ok) return { ok: false, error: await errorFrom(res) };

		const data = await res.json();
		storeSession(server, data, email.trim());

		if (data.encrypted_key_bundle && data.bundle_salt) {
			try {
				const secretKey = await unwrapSecret(
					data.encrypted_key_bundle,
					data.bundle_salt,
					password
				);
				const publicKey = await fetchOwnPublicKey(server, data.user_id, data.token);
				setKeypair({ publicKey, secretKey });
			} catch {
				return {
					ok: true,
					error: 'Signed in, but your encryption key could not be unlocked on this device.',
				};
			}
		}

		return { ok: true };
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Sign-in failed' };
	}
}

export async function resendVerification(email: string): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };
	const res = await fetch(`${server}/api/auth/resend-verification`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ email: email.trim() }),
	});
	return res.ok ? { ok: true } : { ok: false, error: await errorFrom(res) };
}

export async function verifyEmail(token: string): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };
	const res = await fetch(`${server}/api/auth/verify`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ token }),
	});
	if (!res.ok) return { ok: false, error: await errorFrom(res) };

	const server_ = server;
	auth.update((s) => {
		const existing = s.servers[server_];
		if (!existing) return s;
		return { ...s, servers: { ...s.servers, [server_]: { ...existing, emailVerified: true } } };
	});
	return { ok: true };
}

/**
 * Always reports success: the response is identical for a known and an unknown address, so
 * echoing "no such account" would turn the form into a membership oracle.
 */
export async function requestPasswordReset(email: string): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };
	const res = await fetch(`${server}/api/auth/password-reset`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ email: email.trim() }),
	});
	if (!res.ok && res.status !== 404) return { ok: false, error: await errorFrom(res) };
	return { ok: true };
}

export interface ResetInput {
	token: string;
	password: string;
	/** The 12-word code. Omitted means the old key material is written off. */
	recoveryCode?: string;
}

/**
 * With the recovery code the existing x25519 secret is re-wrapped under the new password and the
 * identity key survives. Without it that secret is unrecoverable, so a fresh keypair must be
 * published in the same request or correspondents keep encrypting to the dead key.
 */
export async function confirmPasswordReset(input: ResetInput): Promise<AuthResult> {
	const server = getActiveServer();
	if (!server) return { ok: false, error: 'No server selected' };

	const body: Record<string, unknown> = {
		token: input.token,
		password_length: input.password.length,
	};

	let mintedRecoveryCode: string | undefined;

	try {
		const authSalt = await generateSaltB64();
		body.auth_salt = authSalt;
		body.verifier = await deriveVerifier(input.password, authSalt);

		const code = input.recoveryCode ? normalizeRecoveryCode(input.recoveryCode) : '';

		if (code) {
			const res = await fetch(
				`${server}/api/auth/password-reset/bundle?token=${encodeURIComponent(input.token)}`
			);
			if (!res.ok) return { ok: false, error: await errorFrom(res) };
			const stored = await res.json();
			if (!stored.encrypted_recovery_bundle || !stored.recovery_bundle_salt) {
				return { ok: false, error: 'This account has no recovery code on file.' };
			}

			let secretKey: string;
			try {
				secretKey = await unwrapSecret(
					stored.encrypted_recovery_bundle,
					stored.recovery_bundle_salt,
					code
				);
			} catch {
				return { ok: false, error: 'That recovery code does not match this account.' };
			}

			const wrapped = await wrapSecret(secretKey, input.password);
			body.encrypted_key_bundle = wrapped.bundle;
			body.bundle_salt = wrapped.salt;
		} else {
			const keypair = await generateIdentityKeypair();
			const wrapped = await wrapSecret(keypair.secretKey, input.password);
			mintedRecoveryCode = await generateRecoveryCode();
			const recoveryWrapped = await wrapSecret(keypair.secretKey, mintedRecoveryCode);

			body.encryption_public_key = keypair.publicKey;
			body.encrypted_key_bundle = wrapped.bundle;
			body.bundle_salt = wrapped.salt;
			body.encrypted_recovery_bundle = recoveryWrapped.bundle;
			body.recovery_bundle_salt = recoveryWrapped.salt;
		}

		const res = await fetch(`${server}/api/auth/password-reset/confirm`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(body),
		});
		if (!res.ok) return { ok: false, error: await errorFrom(res) };

		// Every session was revoked server-side, including any this browser held.
		lockAuth();
		auth.update((s) => ({ ...s, servers: {} }));

		return { ok: true, recoveryCode: mintedRecoveryCode };
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Reset failed' };
	}
}

async function currentEmail(server: string, token: string): Promise<string | null> {
	const known = getActiveAuth()?.email;
	if (known) return known;
	const res = await fetch(`${server}/api/auth/me`, {
		headers: { Authorization: `Bearer ${token}` },
	});
	if (!res.ok) return null;
	const data = await res.json();
	return data.email || null;
}

/**
 * The x25519 secret is re-wrapped, not regenerated, so the account reads exactly what it read
 * before; the existing recovery code wraps the same secret and survives too.
 */
export async function changePassword(
	currentPassword: string,
	newPassword: string
): Promise<AuthResult> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return { ok: false, error: 'Not signed in' };

	try {
		const email = await currentEmail(server, token);
		if (!email) return { ok: false, error: 'Could not read your account details' };

		const currentSalt = await fetchAuthSalt(server, email);
		if (!currentSalt) return { ok: false, error: 'Could not start the change. Try again.' };
		const currentVerifier = await deriveVerifier(currentPassword, currentSalt);

		const authSalt = await generateSaltB64();
		const verifier = await deriveVerifier(newPassword, authSalt);

		const body: Record<string, unknown> = {
			current_verifier: currentVerifier,
			verifier,
			auth_salt: authSalt,
			password_length: newPassword.length,
		};

		const secretKey = getEncryptionSecretKey();
		if (secretKey) {
			const wrapped = await wrapSecret(secretKey, newPassword);
			body.encrypted_key_bundle = wrapped.bundle;
			body.bundle_salt = wrapped.salt;
		}

		const res = await fetch(`${server}/api/auth/password/change`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
			body: JSON.stringify(body),
		});
		if (!res.ok) return { ok: false, error: await errorFrom(res) };

		return { ok: true };
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Change failed' };
	}
}

/**
 * Writing the new wrapping over the old is the whole revocation: the server holds nothing derived
 * from the previous code, so no bundle the old code can open remains.
 */
export async function reissueRecoveryCode(currentPassword: string): Promise<AuthResult> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return { ok: false, error: 'Not signed in' };

	const secretKey = getEncryptionSecretKey();
	if (!secretKey) {
		return {
			ok: false,
			error: 'Your encryption key is locked on this device. Sign in again first.',
		};
	}

	try {
		const email = await currentEmail(server, token);
		if (!email) return { ok: false, error: 'Could not read your account details' };
		const currentSalt = await fetchAuthSalt(server, email);
		if (!currentSalt) return { ok: false, error: 'Could not start the change. Try again.' };

		const recoveryCode = await generateRecoveryCode();
		const wrapped = await wrapSecret(secretKey, recoveryCode);

		const res = await fetch(`${server}/api/auth/recovery/reissue`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
			body: JSON.stringify({
				current_verifier: await deriveVerifier(currentPassword, currentSalt),
				encrypted_recovery_bundle: wrapped.bundle,
				recovery_bundle_salt: wrapped.salt,
			}),
		});
		if (!res.ok) return { ok: false, error: await errorFrom(res) };

		return { ok: true, recoveryCode };
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Reissue failed' };
	}
}

export interface SessionSummary {
	id: string;
	device_label: string | null;
	ip: string | null;
	created_at: string;
	last_used_at: string;
	expires_at: string;
	current: boolean;
}

export async function listSessions(): Promise<SessionSummary[]> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return [];
	const res = await fetch(`${server}/api/auth/sessions`, {
		headers: { Authorization: `Bearer ${token}` },
	});
	if (!res.ok) return [];
	return await res.json();
}

export async function revokeSession(id: string): Promise<boolean> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return false;
	const res = await fetch(`${server}/api/auth/sessions/${id}`, {
		method: 'DELETE',
		headers: { Authorization: `Bearer ${token}` },
	});
	return res.ok;
}

export async function revokeOtherSessions(): Promise<number> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return 0;
	const res = await fetch(`${server}/api/auth/sessions`, {
		method: 'DELETE',
		headers: { Authorization: `Bearer ${token}` },
	});
	if (!res.ok) return 0;
	const data = await res.json();
	return data.revoked ?? 0;
}

export async function updateDisplayName(newName: string): Promise<boolean> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return false;

	try {
		const res = await fetch(`${server}/api/auth/me`, {
			method: 'PUT',
			headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
			body: JSON.stringify({ display_name: newName }),
		});

		if (!res.ok) return false;
		auth.update((s) => ({
			...s,
			servers: {
				...s.servers,
				[server]: { ...s.servers[server], displayName: newName },
			},
		}));
		return true;
	} catch {
		return false;
	}
}

export async function refreshRole(): Promise<void> {
	const server = getActiveServer();
	const token = getToken();
	if (!server || !token) return;
	try {
		const res = await fetch(`${server}/api/auth/me`, {
			headers: { Authorization: `Bearer ${token}` },
		});
		if (!res.ok) return;
		const data = await res.json();
		auth.update((s) => {
			const existing = s.servers[server];
			if (!existing) return s;
			return {
				...s,
				servers: {
					...s.servers,
					[server]: {
						...existing,
						role: data.role || existing.role,
						email: data.email || existing.email,
						emailVerified: data.email_verified === true,
					},
				},
			};
		});
	} catch {
		// a failed refresh keeps the last known role; the server re-checks on every request anyway
	}
}

/** Sign out of the active server, telling it to drop the session rather than just forgetting it. */
export function logout() {
	const server = getActiveServer();
	if (!server) return;
	const token = getToken();
	if (token && typeof fetch !== 'undefined') {
		// Fire-and-forget: the local state goes either way, and a failed call only means the
		// session expires on its own schedule instead of now.
		try {
			fetch(`${server}/api/auth/signout`, {
				method: 'POST',
				headers: { Authorization: `Bearer ${token}` },
			}).catch(() => {});
		} catch {
			// no network here; the local sign-out below still happens
		}
	}
	if (typeof sessionStorage !== 'undefined') sessionStorage.removeItem(SESSION_KEY);
	auth.update((s) => {
		const { [server]: _removed, ...rest } = s.servers;
		return { keypair: null, servers: rest };
	});
}

/** Run `action` if signed in, otherwise send the visitor to the sign-in page. */
export function requireAuth(action: () => void) {
	if (isAuthenticated()) {
		action();
		return;
	}
	goto('/account/login');
}
