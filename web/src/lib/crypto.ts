import init, {
	generate_x25519_keypair,
	derive_shared_key,
	encrypt_with_shared_key,
	decrypt_with_shared_key,
	generate_salt,
	derive_key_from_password,
	encrypt_key_bundle,
	decrypt_key_bundle,
	generate_recovery_code,
} from 'komun-wasm';

let initialized = false;

async function ensureInit() {
	if (!initialized) {
		await init();
		initialized = true;
	}
}

/**
 * The only long-lived secret an account has: an x25519 pair so other people can encrypt messages
 * to you.
 */
export interface IdentityKeypair {
	publicKey: string;
	secretKey: string;
}

export function bytesToBase64(bytes: Uint8Array): string {
	return btoa(String.fromCharCode(...bytes));
}

function base64ToBytes(b64: string): Uint8Array {
	const binary = atob(b64);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i++) {
		bytes[i] = binary.charCodeAt(i);
	}
	return bytes;
}

export async function generateIdentityKeypair(): Promise<IdentityKeypair> {
	await ensureInit();
	const kp = generate_x25519_keypair();
	return {
		publicKey: bytesToBase64(new Uint8Array(kp.public_key)),
		secretKey: bytesToBase64(new Uint8Array(kp.secret_key)),
	};
}

/** A fresh 16-byte salt, base64. Salts are public; only the password they stretch is not. */
export async function generateSaltB64(): Promise<string> {
	await ensureInit();
	return bytesToBase64(new Uint8Array(generate_salt()));
}

/**
 * `Argon2id(password, auth_salt)` — the value sent to the server in place of the password, and the
 * only password-derived value that ever leaves the browser. The server re-stretches it under its
 * own salt before storing it.
 */
export async function deriveVerifier(password: string, authSaltB64: string): Promise<string> {
	await ensureInit();
	const derived = derive_key_from_password(
		new TextEncoder().encode(password),
		base64ToBytes(authSaltB64)
	);
	return bytesToBase64(new Uint8Array(derived));
}

/**
 * Seal the x25519 secret under a key derived from `secret` (a password or the recovery code — the
 * wrapping is identical). Returns the base64 ciphertext and salt; the derived key is discarded.
 */
export async function wrapSecret(
	x25519SecretB64: string,
	secret: string
): Promise<{ bundle: string; salt: string }> {
	await ensureInit();
	const salt = new Uint8Array(generate_salt());
	const derivedKey = derive_key_from_password(new TextEncoder().encode(secret), salt);
	const encrypted = encrypt_key_bundle(
		base64ToBytes(x25519SecretB64),
		new Uint8Array(derivedKey)
	);
	return {
		bundle: bytesToBase64(new Uint8Array(encrypted)),
		salt: bytesToBase64(salt),
	};
}

/**
 * Inverse of {@link wrapSecret}. Throws if the secret is wrong or the bundle was tampered with —
 * the AEAD tag fails before anything is returned. The wasm side rejects any plaintext that is not
 * exactly the 32-byte x25519 secret.
 */
export async function unwrapSecret(
	bundleB64: string,
	saltB64: string,
	secret: string
): Promise<string> {
	await ensureInit();
	const derivedKey = derive_key_from_password(
		new TextEncoder().encode(secret),
		base64ToBytes(saltB64)
	);
	const decrypted = decrypt_key_bundle(base64ToBytes(bundleB64), new Uint8Array(derivedKey));
	return bytesToBase64(new Uint8Array(decrypted));
}

/**
 * A 12-word BIP39 phrase, generated in the browser and shown once. The server stores only the
 * x25519 secret wrapped under it, so losing it is unrecoverable.
 */
export async function generateRecoveryCode(): Promise<string> {
	await ensureInit();
	return generate_recovery_code();
}

/** Collapse whitespace and case so a code typed back in with odd spacing still matches. */
export function normalizeRecoveryCode(code: string): string {
	return code.trim().toLowerCase().split(/\s+/).join(' ');
}

export async function deriveConversationKey(
	mySecretKeyBase64: string,
	theirPublicKeyBase64: string
): Promise<string> {
	await ensureInit();
	const mySecret = base64ToBytes(mySecretKeyBase64);
	const theirPublic = base64ToBytes(theirPublicKeyBase64);
	const key = derive_shared_key(mySecret, theirPublic);
	return bytesToBase64(new Uint8Array(key));
}

export async function encryptMessage(
	plaintext: string,
	sharedKeyBase64: string
): Promise<string> {
	await ensureInit();
	const plaintextBytes = new TextEncoder().encode(plaintext);
	const key = base64ToBytes(sharedKeyBase64);
	const encrypted = encrypt_with_shared_key(plaintextBytes, key);
	return bytesToBase64(new Uint8Array(encrypted));
}

export async function decryptMessage(
	encryptedBase64: string,
	sharedKeyBase64: string
): Promise<string> {
	await ensureInit();
	const data = base64ToBytes(encryptedBase64);
	const key = base64ToBytes(sharedKeyBase64);
	const plaintext = decrypt_with_shared_key(data, key);
	return new TextDecoder().decode(new Uint8Array(plaintext));
}
