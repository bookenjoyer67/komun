import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, within, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { auth } from '$lib/stores/auth';
import { serverState } from '$lib/stores/server';
import AccountPage from '../routes/account/+page.svelte';
import AdminPage from '../routes/admin/+page.svelte';
import PostPage from '../routes/p/[id]/+page.svelte';

/**
 * A removal is never silent: its author sees the reason it was given and may appeal it once,
 * inside the window; a moderator answers from a queue, and a denial carries a note. The stored
 * status words never reach a reader, and a hidden post never offers a respond control.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-1';
const APPEAL_ID = 'appeal-1';
const AUTHOR = 'user-author';
const REASON = 'A duplicate of an earlier post';
const APPEAL_TEXT = 'The earlier post was withdrawn last week';
const NOTE = 'The earlier post is still listed';
const DAY = 86_400_000;
const STORED_WORDS = ['hidden', 'pending', 'granted', 'denied'];

const fixture = vi.hoisted(() => ({
	post: {} as Record<string, unknown>
}));

vi.mock('$app/stores', async () => {
	const { writable } = await import('svelte/store');
	return {
		page: writable({ params: { id: 'post-1' }, url: new URL('http://localhost/p/post-1') }),
		navigating: writable(null),
		updated: writable(false)
	};
});

vi.mock('$lib/api/client', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/client')>();
	return {
		...actual,
		api: {
			...actual.api,
			posts: { ...actual.api.posts, get: async () => ({ ...fixture.post }) }
		}
	};
});

interface Call {
	url: string;
	method: string;
	body: unknown;
}

let calls: Call[] = [];
let notices: Record<string, unknown>[] = [];
let appeals: Record<string, unknown>[] = [];
let answer: ((url: string, method: string) => Response | undefined) | null = null;

function json(data: unknown, status = 200): Response {
	return new Response(JSON.stringify(data), {
		status,
		headers: { 'Content-Type': 'application/json' }
	});
}

function route(url: string, method: string): Response {
	const special = answer?.(url, method);
	if (special) return special;
	if (url.endsWith('/api/me/moderation')) return json(notices);
	if (url.endsWith('/api/admin/appeals')) return json(appeals);
	if (url.endsWith(`/api/posts/${POST_ID}/appeal`)) return json({ id: APPEAL_ID }, 201);
	if (url.endsWith(`/api/admin/appeals/${APPEAL_ID}`)) return json({ status: 'ok' });
	if (url.endsWith('/api/admin/stats')) {
		return json({ users: 1, active_posts: 1, total_posts: 1, matches: 0, messages: 0, directory_entries: 0 });
	}
	return json({ error: 'not found' }, 404);
}

function sent(method: string, path: string): Call[] {
	return calls.filter((c) => c.method === method && c.url.endsWith(path));
}

const makeNotice = (overrides: Record<string, unknown> = {}) => ({
	post_id: POST_ID,
	post_title: 'Need groceries',
	action_id: 'action-1',
	reason: REASON,
	hidden_at: new Date(Date.now() - DAY).toISOString(),
	appeal_deadline: new Date(Date.now() + 13 * DAY).toISOString(),
	appeal_open: true,
	appeal_status: null,
	appeal_admin_notes: null,
	...overrides
});

const makeAppeal = (overrides: Record<string, unknown> = {}) => ({
	id: APPEAL_ID,
	post_id: POST_ID,
	post_title: 'Need groceries',
	author_id: AUTHOR,
	author_name: 'Test Author',
	reason: REASON,
	body: APPEAL_TEXT,
	status: 'pending',
	admin_notes: null,
	created_at: new Date(Date.now() - DAY).toISOString(),
	resolved_at: null,
	...overrides
});

function signInAs(userId: string, role = 'user') {
	auth.set({
		keypair: null,
		servers: { [SERVER]: { token: 'fake-test-token', userId, displayName: 'Test User', role } }
	});
}

function expectNoStoredWords(element: HTMLElement) {
	const text = (element.textContent ?? '').toLowerCase();
	for (const word of STORED_WORDS) expect(text).not.toContain(word);
}

async function removedPosts(): Promise<HTMLElement> {
	render(AccountPage);
	const region = await screen.findByRole('region', { name: 'Removed posts' });
	await waitFor(() => expect(within(region).queryByText(/Checking/)).not.toBeInTheDocument());
	return region;
}

async function appealQueue(): Promise<HTMLElement> {
	render(AdminPage);
	const region = await screen.findByRole('region', { name: 'Appeals' });
	await waitFor(() => expect(within(region).queryByText(/Loading appeals/)).not.toBeInTheDocument());
	return region;
}

beforeEach(() => {
	calls = [];
	notices = [];
	appeals = [];
	answer = null;
	vi.stubGlobal(
		'fetch',
		vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			const url = String(input);
			const method = init?.method ?? 'GET';
			const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
			calls.push({ url, method, body });
			return route(url, method);
		})
	);
	vi.stubGlobal(
		'matchMedia',
		vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }))
	);
	serverState.set({ active: SERVER, known: [] });
});

afterEach(() => {
	vi.unstubAllGlobals();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
});

describe('account page: the author is told why a post was removed', () => {
	it('shows the reason and an appeal box while the window is open', async () => {
		signInAs(AUTHOR);
		notices = [makeNotice()];

		const region = await removedPosts();

		expect(within(region).getByRole('heading', { name: 'Need groceries' })).toBeInTheDocument();
		expect(within(region).getByText(REASON)).toBeInTheDocument();
		expect(within(region).getByLabelText('Your appeal')).toBeInTheDocument();
		expect(within(region).getByRole('button', { name: 'Send appeal' })).toBeEnabled();
		expectNoStoredWords(region);
	});

	it('offers no respond control beside a removed post', async () => {
		signInAs(AUTHOR);
		notices = [makeNotice()];

		const region = await removedPosts();

		const respond = /i can help|request this|make an offer|respond/i;
		expect(within(region).queryByRole('button', { name: respond })).not.toBeInTheDocument();
	});

	it('sends the appeal to the post and then says it is with a moderator', async () => {
		const user = userEvent.setup();
		signInAs(AUTHOR);
		notices = [makeNotice()];
		const region = await removedPosts();

		await user.type(within(region).getByLabelText('Your appeal'), APPEAL_TEXT);
		await user.click(within(region).getByRole('button', { name: 'Send appeal' }));

		expect(await within(region).findByText('Your appeal is with a moderator.')).toBeInTheDocument();
		const posted = sent('POST', `/api/posts/${POST_ID}/appeal`);
		expect(posted).toHaveLength(1);
		expect(posted[0].body).toEqual({ body: APPEAL_TEXT });
		expect(within(region).queryByRole('button', { name: 'Send appeal' })).not.toBeInTheDocument();
		expectNoStoredWords(region);
	});

	it('refuses an empty appeal on the page and sends nothing', async () => {
		const user = userEvent.setup();
		signInAs(AUTHOR);
		notices = [makeNotice()];
		const region = await removedPosts();

		await user.click(within(region).getByRole('button', { name: 'Send appeal' }));

		expect(await within(region).findByRole('alert')).toHaveTextContent('Say why the post should come back');
		expect(sent('POST', `/api/posts/${POST_ID}/appeal`)).toHaveLength(0);
	});

	it("shows the server's refusal when a second appeal is turned away", async () => {
		const user = userEvent.setup();
		signInAs(AUTHOR);
		notices = [makeNotice()];
		answer = (url, method) =>
			method === 'POST' && url.endsWith(`/api/posts/${POST_ID}/appeal`)
				? json({ error: 'this removal has already been appealed' }, 409)
				: undefined;
		const region = await removedPosts();

		await user.type(within(region).getByLabelText('Your appeal'), APPEAL_TEXT);
		await user.click(within(region).getByRole('button', { name: 'Send appeal' }));

		expect(await within(region).findByRole('alert')).toHaveTextContent('already been appealed');
		expect(within(region).getByRole('button', { name: 'Send appeal' })).toBeEnabled();
	});

	it('offers no appeal box once the window has closed', async () => {
		signInAs(AUTHOR);
		notices = [makeNotice({ appeal_open: false, appeal_deadline: new Date(Date.now() - DAY).toISOString() })];

		const region = await removedPosts();

		expect(within(region).getByText(REASON)).toBeInTheDocument();
		expect(within(region).getByText(/The window to appeal this closed on/)).toBeInTheDocument();
		expect(within(region).queryByLabelText('Your appeal')).not.toBeInTheDocument();
		expectNoStoredWords(region);
	});

	it("shows a declined appeal with the moderator's note and no second appeal box", async () => {
		signInAs(AUTHOR);
		notices = [makeNotice({ appeal_open: false, appeal_status: 'denied', appeal_admin_notes: NOTE })];

		const region = await removedPosts();

		expect(within(region).getByText('Your appeal was declined.')).toBeInTheDocument();
		expect(within(region).getByText(NOTE)).toBeInTheDocument();
		expect(within(region).queryByLabelText('Your appeal')).not.toBeInTheDocument();
		expectNoStoredWords(region);
	});

	it('says plainly when nothing has been removed', async () => {
		signInAs(AUTHOR);

		const region = await removedPosts();

		expect(within(region).getByText('None of your posts has been removed from public view.')).toBeInTheDocument();
	});

	it('says so when the removals cannot be read', async () => {
		signInAs(AUTHOR);
		answer = (url) => (url.endsWith('/api/me/moderation') ? json({ error: 'boom' }, 500) : undefined);

		const region = await removedPosts();

		expect(within(region).getByRole('alert')).toHaveTextContent('Could not check for removed posts');
	});
});

describe('admin page: the appeals queue', () => {
	it("lists a waiting appeal with the reason and the author's words", async () => {
		signInAs('user-admin', 'superadmin');
		appeals = [makeAppeal()];

		const region = await appealQueue();

		expect(within(region).getByRole('link', { name: 'Need groceries' })).toHaveAttribute('href', `/p/${POST_ID}`);
		expect(within(region).getByText(REASON)).toBeInTheDocument();
		expect(within(region).getByText(APPEAL_TEXT)).toBeInTheDocument();
		expect(within(region).getByRole('button', { name: 'Restore post' })).toBeEnabled();
		expect(within(region).getByRole('button', { name: 'Decline appeal' })).toBeEnabled();
		expectNoStoredWords(region);
	});

	it('restoring sends granted to the appeal and moves it to the decided list', async () => {
		const user = userEvent.setup();
		signInAs('user-admin', 'superadmin');
		appeals = [makeAppeal()];
		const region = await appealQueue();

		await user.click(within(region).getByRole('button', { name: 'Restore post' }));

		expect(await within(region).findByText('Post restored')).toBeInTheDocument();
		const patched = sent('PATCH', `/api/admin/appeals/${APPEAL_ID}`);
		expect(patched).toHaveLength(1);
		expect(patched[0].body).toEqual({ status: 'granted', admin_notes: null });
		expect(within(region).getByText('No appeals are waiting for a decision.')).toBeInTheDocument();
		expectNoStoredWords(region);
	});

	it('declining without a note is refused on the page and sends nothing', async () => {
		const user = userEvent.setup();
		signInAs('user-admin', 'superadmin');
		appeals = [makeAppeal()];
		const region = await appealQueue();

		await user.click(within(region).getByRole('button', { name: 'Decline appeal' }));

		expect(await within(region).findByRole('alert')).toHaveTextContent('Write a note to the author');
		expect(sent('PATCH', `/api/admin/appeals/${APPEAL_ID}`)).toHaveLength(0);
	});

	it('declining with a note sends the note to the author', async () => {
		const user = userEvent.setup();
		signInAs('user-admin', 'superadmin');
		appeals = [makeAppeal()];
		const region = await appealQueue();

		await user.type(within(region).getByLabelText('Note to the author'), NOTE);
		await user.click(within(region).getByRole('button', { name: 'Decline appeal' }));

		expect(await within(region).findByText('Appeal declined')).toBeInTheDocument();
		const patched = sent('PATCH', `/api/admin/appeals/${APPEAL_ID}`);
		expect(patched).toHaveLength(1);
		expect(patched[0].body).toEqual({ status: 'denied', admin_notes: NOTE });
		expectNoStoredWords(region);
	});

	it('says when no appeal is waiting', async () => {
		signInAs('user-admin', 'superadmin');

		const region = await appealQueue();

		expect(within(region).getByText('No appeals are waiting for a decision.')).toBeInTheDocument();
	});
});

describe('post page: a hidden post offers no respond control', () => {
	it('shows a stranger who can still read it no way to respond', async () => {
		signInAs('user-stranger');
		fixture.post = {
			id: POST_ID,
			author_id: AUTHOR,
			kind: 'need',
			category: 'food',
			title: 'Need groceries',
			body: 'Can someone help with groceries this week?',
			status: 'hidden',
			created_at: new Date(Date.now() - DAY).toISOString()
		};

		render(PostPage);
		await screen.findByRole('heading', { name: 'Need groceries' });

		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
		expect((document.body.textContent ?? '').toLowerCase()).not.toContain('hidden');
	});
});
