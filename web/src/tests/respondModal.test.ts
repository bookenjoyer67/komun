import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { encrypt_with_shared_key } from 'komun-wasm';
import { serverState } from '$lib/stores/server';
import { auth } from '$lib/stores/auth';
import { bytesToBase64 } from '$lib/crypto';
import { RESPONSE_PREFIX, encodeResponse, decodeMessage } from '$lib/messageFormat';
import RespondModal from '$lib/components/RespondModal.svelte';

/**
 * Structured respond (marketplace-parity Wave 1). Only `fetch` is stubbed; the real API client,
 * stores, crypto wrappers and envelope encoder run. Every key and sentence is a fake test value.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-respond';
const AUTHOR = 'user-author';

const KEYS_URL = `${SERVER}/api/auth/users/${AUTHOR}/keys`;
const RESPOND_URL = `${SERVER}/api/posts/${POST_ID}/respond`;

const POST = { id: POST_ID, title: 'Test need', kind: 'need', server_url: SERVER, author_id: AUTHOR };

let fetchMock: ReturnType<typeof vi.fn>;

function reply(status: number, body: unknown): Response {
	return {
		ok: status >= 200 && status < 300,
		status,
		statusText: status === 200 ? 'OK' : 'Not Found',
		json: async () => body
	} as unknown as Response;
}

function installServer() {
	fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
		const url = String(input);
		if (url === KEYS_URL) {
			return reply(200, { encryption_public_key: bytesToBase64(new Uint8Array(32).fill(5)) });
		}
		if (url === RESPOND_URL && init?.method === 'POST') return reply(200, { match_id: 'match-new' });
		return reply(404, { error: 'unexpected request in test' });
	});
	vi.stubGlobal('fetch', fetchMock);
}

function respondCalls() {
	return fetchMock.mock.calls.filter(([input]) => String(input) === RESPOND_URL);
}

/** The plaintext handed to the seal, read back from the wasm stand-in's input. */
function sealedPlaintext(): string {
	const input = vi.mocked(encrypt_with_shared_key).mock.calls[0][0] as Uint8Array;
	return new TextDecoder().decode(input);
}

beforeEach(() => {
	vi.clearAllMocks();
	vi.mocked(encrypt_with_shared_key).mockImplementation(((data: Uint8Array) =>
		data.slice().buffer) as never);
	serverState.set({ active: SERVER, known: [] });
	auth.set({
		keypair: {
			publicKey: bytesToBase64(new Uint8Array(32).fill(7)),
			secretKey: bytesToBase64(new Uint8Array(32).fill(9))
		},
		servers: {
			[SERVER]: { token: 'fake-test-token', userId: 'user-me', displayName: 'Test Me', role: 'user' }
		}
	});
	installServer();
});

afterEach(() => {
	vi.unstubAllGlobals();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
});

describe('RespondModal structured respond', () => {
	it('refuses a response with an empty "what" and sends nothing', async () => {
		const user = userEvent.setup();
		render(RespondModal, { props: { post: POST, onClose: vi.fn() } });

		await user.type(screen.getByLabelText(/^When/), 'Saturday morning');
		await user.type(screen.getByLabelText(/^Anything else/), 'only a note');
		await user.click(screen.getByRole('button', { name: 'Send Response' }));

		expect(await screen.findByRole('alert')).toHaveTextContent('Say what you can offer');
		expect(respondCalls()).toHaveLength(0);
		expect(encrypt_with_shared_key).not.toHaveBeenCalled();
	});

	it('leaves "when" optional: a response with only "what" is sealed and sent', async () => {
		const user = userEvent.setup();
		render(RespondModal, { props: { post: POST, onClose: vi.fn() } });

		await user.type(screen.getByLabelText('What you can offer'), 'A ladder');
		await user.click(screen.getByRole('button', { name: 'Send Response' }));

		await waitFor(() => expect(respondCalls()).toHaveLength(1));
		expect(decodeMessage(sealedPlaintext())).toEqual({ kind: 'response', what: 'A ladder' });
	});

	it('seals the output of encodeResponse, and the sealed plaintext starts with the envelope prefix', async () => {
		const user = userEvent.setup();
		render(RespondModal, { props: { post: POST, onClose: vi.fn() } });

		await user.type(screen.getByLabelText('What you can offer'), 'A ladder and two hours');
		await user.type(screen.getByLabelText(/^When/), 'Saturday morning');
		await user.type(screen.getByLabelText(/^Anything else/), 'I live nearby.');
		await user.click(screen.getByRole('button', { name: 'Send Response' }));

		await waitFor(() => expect(respondCalls()).toHaveLength(1));
		expect(encrypt_with_shared_key).toHaveBeenCalledTimes(1);
		const plaintext = sealedPlaintext();
		expect(plaintext.startsWith(RESPONSE_PREFIX)).toBe(true);
		expect(plaintext).toBe(
			encodeResponse({ what: 'A ladder and two hours', when: 'Saturday morning', note: 'I live nearby.' })
		);
		const sent = JSON.parse(String(respondCalls()[0][1]?.body)).ciphertext;
		expect(sent).toBe(bytesToBase64(new TextEncoder().encode(plaintext)));
	});
});
