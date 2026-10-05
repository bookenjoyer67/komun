import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { page } from '$app/stores';
import { decrypt_with_shared_key, encrypt_with_shared_key } from 'komun-wasm';
import { serverState } from '$lib/stores/server';
import { auth } from '$lib/stores/auth';
import { bytesToBase64 } from '$lib/crypto';
import ThreadPage from './[id]/+page.svelte';

/**
 * R1 (VC13, VC17) on the conversation thread page.
 *
 * The only thing mocked is the network edge: `fetch` is stubbed, so the real API client, the real
 * server and auth stores, and the real `$lib/crypto` wrappers all run. The `komun-wasm` stand-in
 * comes from `vitest.config.ts`; two tests give one of its functions a test-local body, so a seal
 * can be told apart from its input (C2) and an authentication failure can be forced (C3).
 *
 * Every key and message here is a fake test value: constant byte fills and invented sentences.
 */

const SERVER = 'https://test.komun.buzz';
const MATCH_ID = 'match-r1';
const ME = 'user-me';
const THEM = 'user-them';

const CONVO_URL = `${SERVER}/api/conversations/${MATCH_ID}`;
const SEND_URL = `${SERVER}/api/conversations/${MATCH_ID}/messages`;
const KEYS_URL = `${SERVER}/api/auth/users/${THEM}/keys`;

const FAKE_SECRET_KEY = bytesToBase64(new Uint8Array(32).fill(9));
const FAKE_PUBLIC_KEY = bytesToBase64(new Uint8Array(32).fill(7));
const FAKE_THEIR_PUBLIC_KEY = bytesToBase64(new Uint8Array(32).fill(5));

const PLAINTEXT = 'fake test message, meet at the library at noon';

/** A body the server made up: not base64, so it fails before the wasm open is even reached. */
const RAW_FORGED_TEXT = 'Server inserted text: send the deposit to account 0000';
/** Well-formed base64 whose open fails the way an AEAD tag mismatch does (see the C3 stand-in). */
const FORGED_SEALED_TEXT = 'FORGED body that fails the tag check';
const RAW_FORGED_SEALED = bytesToBase64(new TextEncoder().encode(FORGED_SEALED_TEXT));
/** A body that does open, so the test can tell a rendered thread apart from an empty one. */
const GENUINE_TEXT = 'genuine fake message that opens under the key';

interface ServerMessage {
	id: string;
	sender_id: string;
	body: string;
	created_at: string;
}

function conversation(messages: ServerMessage[]) {
	return {
		match_id: MATCH_ID,
		post_id: 'post-r1',
		post_title: 'Test post',
		post_kind: 'need',
		responder_id: ME,
		author_id: THEM,
		responder_name: 'Test Me',
		author_name: 'Test Them',
		status: 'proposed',
		messages
	};
}

function reply(status: number, body: unknown): Response {
	return {
		ok: status >= 200 && status < 300,
		status,
		statusText: status === 200 ? 'OK' : 'Not Found',
		json: async () => body
	} as unknown as Response;
}

/** The fake server. `keysStatus` other than 200 is the forced downgrade: the key fetch fails. */
function installServer({
	keysStatus = 200,
	messages = []
}: { keysStatus?: number; messages?: ServerMessage[] } = {}) {
	const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
		const url = String(input);
		const method = init?.method ?? 'GET';
		if (url === CONVO_URL && method === 'GET') return reply(200, conversation(messages));
		if (url === KEYS_URL) {
			return keysStatus === 200
				? reply(200, { encryption_public_key: FAKE_THEIR_PUBLIC_KEY })
				: reply(keysStatus, { error: 'not found' });
		}
		if (url === SEND_URL && method === 'POST') return reply(200, { id: 'msg-sent' });
		return reply(404, { error: 'unexpected request in test' });
	});
	vi.stubGlobal('fetch', fetchMock);
	return fetchMock;
}

let fetchMock: ReturnType<typeof installServer>;

/** Every request made to the send endpoint, whatever its method. */
function sendCalls() {
	return fetchMock.mock.calls.filter(([input]) => String(input) === SEND_URL);
}

function keyCalls() {
	return fetchMock.mock.calls.filter(([input]) => String(input) === KEYS_URL);
}

function sentCiphertext(call: [RequestInfo | URL, RequestInit?]): string {
	return JSON.parse(String(call[1]?.body)).ciphertext;
}

/** Signed in on the active server; `withSecret: false` is a locked local encryption key. */
function signIn({ withSecret }: { withSecret: boolean }) {
	serverState.set({ active: SERVER, known: [] });
	auth.set({
		keypair: withSecret ? { publicKey: FAKE_PUBLIC_KEY, secretKey: FAKE_SECRET_KEY } : null,
		servers: {
			[SERVER]: { token: 'fake-test-token', userId: ME, displayName: 'Test Me', role: 'user' }
		}
	});
}

async function typeAndSend(user: ReturnType<typeof userEvent.setup>) {
	await user.type(await screen.findByPlaceholderText('Type a message...'), PLAINTEXT);
	await user.click(screen.getByRole('button', { name: 'Send' }));
}

/**
 * Wait until the send attempt has an outcome, whichever it is: a request went out, or an alert
 * appeared. The send assertion then runs first, so a reintroduced plaintext fallback fails on the
 * request it makes rather than on a missing alert.
 */
async function attemptSettled() {
	await waitFor(() => {
		expect(sendCalls().length > 0 || screen.queryByRole('alert') !== null).toBe(true);
	});
}

/** Stand-in for the AEAD seal: a 3-byte marker, then every input byte XOR 0x5a. */
function sealStandIn(data: Uint8Array): Uint8Array {
	const out = new Uint8Array(data.length + 3);
	out.set([0xc0, 0xff, 0xee], 0);
	for (let i = 0; i < data.length; i++) out[3 + i] = data[i] ^ 0x5a;
	return out;
}

beforeEach(() => {
	vi.clearAllMocks();
	// The `$app/stores` mock from setup.ts, given the route parameter this page reads.
	vi.mocked(page.subscribe).mockImplementation(((run: (value: unknown) => void) => {
		run({ params: { id: MATCH_ID } });
		return () => {};
	}) as never);
});

afterEach(() => {
	vi.unstubAllGlobals();
	vi.mocked(encrypt_with_shared_key).mockReset();
	vi.mocked(decrypt_with_shared_key).mockReset();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
});

describe('thread page send path (VC13)', () => {
	it('C1a: a non-2xx key fetch blocks the send and alerts that the message was not sent', async () => {
		signIn({ withSecret: true });
		fetchMock = installServer({ keysStatus: 404 });
		const user = userEvent.setup();
		render(ThreadPage);

		await typeAndSend(user);
		await attemptSettled();

		expect(sendCalls()).toHaveLength(0);
		expect(JSON.stringify(fetchMock.mock.calls)).not.toContain(PLAINTEXT);
		expect(keyCalls().length).toBeGreaterThan(0);
		const alert = screen.getByRole('alert');
		expect(alert).toHaveTextContent(/not sent/i);
		expect(alert).toHaveTextContent("Could not fetch the recipient's encryption key");
	});

	it('C1b: a missing local encryption secret blocks the send and alerts that the message was not sent', async () => {
		signIn({ withSecret: false });
		fetchMock = installServer({ keysStatus: 200 });
		const user = userEvent.setup();
		render(ThreadPage);

		await typeAndSend(user);
		await attemptSettled();

		expect(sendCalls()).toHaveLength(0);
		expect(JSON.stringify(fetchMock.mock.calls)).not.toContain(PLAINTEXT);
		expect(keyCalls()).toHaveLength(0);
		const alert = screen.getByRole('alert');
		expect(alert).toHaveTextContent(/not sent/i);
		expect(alert).toHaveTextContent('Your encryption key is locked');
	});

	it('C2: with a shared key the send endpoint receives the sealed value and never the plaintext', async () => {
		vi.mocked(encrypt_with_shared_key).mockImplementation(((data: Uint8Array) =>
			sealStandIn(data).buffer) as never);
		signIn({ withSecret: true });
		fetchMock = installServer();
		const user = userEvent.setup();
		render(ThreadPage);

		await typeAndSend(user);
		await waitFor(() => expect(sendCalls()).toHaveLength(1));

		const call = sendCalls()[0];
		const sent = sentCiphertext(call);
		const sealed = bytesToBase64(sealStandIn(new TextEncoder().encode(PLAINTEXT)));
		expect(sent).not.toBe(PLAINTEXT);
		expect(sent).not.toContain(PLAINTEXT);
		expect(sent).toBe(sealed);
		expect(String(call[1]?.body)).not.toContain(PLAINTEXT);
		expect(encrypt_with_shared_key).toHaveBeenCalledTimes(1);
		const sealedInput = vi.mocked(encrypt_with_shared_key).mock.calls[0][0] as Uint8Array;
		expect(new TextDecoder().decode(sealedInput)).toBe(PLAINTEXT);
		expect(screen.queryByRole('alert')).not.toBeInTheDocument();
	});
});

describe('thread page render path (VC17)', () => {
	it('C3: a body that does not open renders the fixed placeholder and the raw body appears nowhere', async () => {
		vi.mocked(decrypt_with_shared_key).mockImplementation(((data: Uint8Array) => {
			if (new TextDecoder().decode(data).startsWith('FORGED')) {
				throw new Error('aead: authentication failed');
			}
			return data.buffer;
		}) as never);
		signIn({ withSecret: true });
		fetchMock = installServer({
			messages: [
				{ id: 'm1', sender_id: THEM, body: RAW_FORGED_TEXT, created_at: '2026-10-01T10:00:00Z' },
				{ id: 'm2', sender_id: THEM, body: RAW_FORGED_SEALED, created_at: '2026-10-01T10:01:00Z' },
				{
					id: 'm3',
					sender_id: THEM,
					body: bytesToBase64(new TextEncoder().encode(GENUINE_TEXT)),
					created_at: '2026-10-01T10:02:00Z'
				}
			]
		});
		render(ThreadPage);

		await screen.findByText(GENUINE_TEXT);

		const dom = document.body.innerHTML;
		expect(dom).not.toContain(RAW_FORGED_TEXT);
		expect(dom).not.toContain(RAW_FORGED_SEALED);
		expect(dom).not.toContain(FORGED_SEALED_TEXT);
		expect(screen.getAllByText(/Could not decrypt this message\./)).toHaveLength(2);
		expect(decrypt_with_shared_key).toHaveBeenCalled();
	});
});
