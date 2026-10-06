import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { page } from '$app/stores';
import { serverState } from '$lib/stores/server';
import { auth, confirmPasswordReset } from '$lib/stores/auth';
import ResetPage from '../routes/account/reset/+page.svelte';

/**
 * The reset token travels in request bodies only: the bundle read is a POST, and the reset page
 * takes the token out of the address bar as soon as it opens, yet still submits it.
 *
 * `fetch` and the crypto wrappers are stubbed; every token, key and password here is a fake value.
 */

const nav = vi.hoisted(() => ({ replaceState: vi.fn(), goto: vi.fn() }));

vi.mock('$app/navigation', () => ({
	goto: nav.goto,
	afterNavigate: vi.fn(),
	beforeNavigate: vi.fn(),
	replaceState: nav.replaceState,
	pushState: vi.fn()
}));

vi.mock('$lib/crypto', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/crypto')>();
	return {
		...actual,
		generateIdentityKeypair: vi.fn(async () => ({
			publicKey: 'fake-public-key',
			secretKey: 'fake-secret-key'
		})),
		generateSaltB64: vi.fn(async () => 'fake-auth-salt'),
		deriveVerifier: vi.fn(async () => 'fake-verifier'),
		wrapSecret: vi.fn(async () => ({ bundle: 'fake-wrapped-bundle', salt: 'fake-wrap-salt' })),
		unwrapSecret: vi.fn(async () => 'fake-secret-key'),
		generateRecoveryCode: vi.fn(async () => 'fake minted recovery code')
	};
});

const SERVER = 'https://test.komun.buzz';
const BUNDLE_URL = `${SERVER}/api/auth/password-reset/bundle`;
const CONFIRM_URL = `${SERVER}/api/auth/password-reset/confirm`;

const TOKEN = 'fake-reset-token-abc';
const PASSWORD = 'fake test password';
const RECOVERY_CODE = 'one two three four five six seven eight nine ten eleven twelve';
const RESET_PATH_WITH_TOKEN = `/account/reset#token=${TOKEN}`;

function reply(status: number, body: unknown): Response {
	return {
		ok: status >= 200 && status < 300,
		status,
		statusText: status === 200 ? 'OK' : 'Not Found',
		json: async () => body
	} as unknown as Response;
}

function installServer() {
	const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
		const url = String(input);
		if (url.startsWith(BUNDLE_URL)) {
			return reply(200, {
				encrypted_recovery_bundle: 'fake-recovery-bundle',
				recovery_bundle_salt: 'fake-recovery-salt'
			});
		}
		if (url === CONFIRM_URL) return reply(200, { ok: true });
		return reply(404, { error: 'unexpected request in test' });
	});
	vi.stubGlobal('fetch', fetchMock);
	return fetchMock;
}

let fetchMock: ReturnType<typeof installServer>;

function callsTo(prefix: string) {
	return fetchMock.mock.calls.filter(([input]) => String(input).startsWith(prefix));
}

/** The page reads its URL from `$page`; the document location is set too, so either source holds the token. */
function openResetPage(path: string) {
	window.history.replaceState(null, '', path);
	const url = new URL(path, window.location.origin);
	vi.mocked(page.subscribe).mockImplementation(((run: (value: unknown) => void) => {
		run({ url, params: {} });
		return () => {};
	}) as never);
	return render(ResetPage);
}

beforeEach(() => {
	vi.clearAllMocks();
	serverState.set({ active: SERVER, known: [] });
	fetchMock = installServer();
});

afterEach(() => {
	vi.unstubAllGlobals();
	window.history.replaceState(null, '', '/');
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
	localStorage.clear();
	sessionStorage.clear();
});

describe('confirmPasswordReset', () => {
	it('reads the recovery bundle by POST with the token in a JSON body, never in a URL', async () => {
		const result = await confirmPasswordReset({
			token: TOKEN,
			password: PASSWORD,
			recoveryCode: RECOVERY_CODE
		});

		expect(result.ok).toBe(true);
		for (const [input] of fetchMock.mock.calls) {
			expect(String(input)).not.toContain('token=');
			expect(String(input)).not.toContain(TOKEN);
		}
		const bundleCalls = callsTo(BUNDLE_URL);
		expect(bundleCalls).toHaveLength(1);
		const init = bundleCalls[0][1];
		expect(init?.method).toBe('POST');
		expect(new Headers(init?.headers).get('Content-Type')).toBe('application/json');
		expect(JSON.parse(String(init?.body))).toEqual({ token: TOKEN });
	});
});

describe('reset page', () => {
	it('takes the token out of the address bar when it opens', async () => {
		openResetPage(RESET_PATH_WITH_TOKEN);

		await waitFor(() => expect(nav.replaceState).toHaveBeenCalled());
		for (const [target] of nav.replaceState.mock.calls) {
			expect(String(target)).not.toContain(TOKEN);
			expect(String(target)).not.toContain('token=');
		}
	});

	it('still submits the token in the confirm body after taking it out of the address bar', async () => {
		const user = userEvent.setup();
		openResetPage(RESET_PATH_WITH_TOKEN);

		await user.type(screen.getByLabelText('New password'), PASSWORD);
		await user.type(screen.getByLabelText('Confirm new password'), PASSWORD);
		await user.click(screen.getByRole('checkbox'));
		await user.click(screen.getByRole('button', { name: 'Reset password' }));

		await waitFor(() => expect(callsTo(CONFIRM_URL)).toHaveLength(1));
		const body = JSON.parse(String(callsTo(CONFIRM_URL)[0][1]?.body));
		expect(body.token).toBe(TOKEN);
	});
});
