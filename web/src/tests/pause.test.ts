import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { auth } from '$lib/stores/auth';
import { serverState } from '$lib/stores/server';
import type { PostLike } from '$lib/api/types';
import PostPage from '../routes/p/[id]/+page.svelte';
import AidPage from '../routes/aid/+page.svelte';
import AidCard from '$lib/components/AidCard.svelte';

/**
 * Pending state. A pause reuses the stored `matched` status, which
 * the server already refuses responses on. Every surface that offers a response must withhold it
 * while a post is paused, and say so in words, never with the API's status value.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-1';
const AUTHOR = 'user-author';
const STRANGER = 'user-stranger';
const PAUSED = 'Paused — not taking responses';

const fixture = vi.hoisted(() => ({
	post: {} as Record<string, unknown>,
	list: [] as Record<string, unknown>[]
}));
const update = vi.hoisted(() => vi.fn());

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
			posts: {
				...actual.api.posts,
				get: async () => ({ ...fixture.post }),
				list: async () => fixture.list.map((p) => ({ ...p })),
				update
			}
		}
	};
});

const makePost = (overrides: Partial<PostLike> = {}): PostLike => ({
	id: POST_ID,
	author_id: AUTHOR,
	kind: 'need',
	category: 'food',
	title: 'Need groceries',
	body: 'Can someone help with groceries this week?',
	status: 'active',
	created_at: new Date(Date.now() - 3600000).toISOString(),
	...overrides
});

function signInAs(userId: string | null) {
	auth.set({
		keypair: null,
		servers: userId
			? { [SERVER]: { token: 'fake-test-token', userId, displayName: 'Test User', role: 'user' } }
			: {}
	});
}

/** The stored value must never reach the reader, in any letter case. */
function expectNoRawStatus() {
	expect((document.body.textContent ?? '').toLowerCase()).not.toContain('matched');
}

async function renderPost(post: PostLike) {
	fixture.post = { ...post };
	render(PostPage);
	await screen.findByRole('heading', { name: post.title });
}

beforeEach(() => {
	update.mockReset();
	update.mockResolvedValue({ status: 'updated' });
	vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
	serverState.set({ active: SERVER, known: [] });
	signInAs(null);
});

afterEach(() => {
	vi.unstubAllGlobals();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
});

describe('post page: the author pauses and resumes responses', () => {
	it('offers the author a Pause control on an active post', async () => {
		signInAs(AUTHOR);
		await renderPost(makePost());

		expect(screen.getByRole('button', { name: 'Pause responses' })).toBeInTheDocument();
		expect(screen.queryByText(PAUSED)).not.toBeInTheDocument();
	});

	it('offers a non-author no Pause or Resume control, only the respond control', async () => {
		signInAs(STRANGER);
		await renderPost(makePost());

		expect(screen.queryByRole('button', { name: /pause|resume/i })).not.toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'I can help' })).toBeInTheDocument();
	});

	it('offers an anonymous reader no Pause or Resume control', async () => {
		await renderPost(makePost());

		expect(screen.queryByRole('button', { name: /pause|resume/i })).not.toBeInTheDocument();
	});

	it('pausing sends status matched through api.posts.update and then shows the paused words', async () => {
		const user = userEvent.setup();
		signInAs(AUTHOR);
		await renderPost(makePost());

		await user.click(screen.getByRole('button', { name: 'Pause responses' }));

		expect(update).toHaveBeenCalledTimes(1);
		expect(update).toHaveBeenCalledWith(POST_ID, { status: 'matched' });
		expect(await screen.findByText(PAUSED)).toBeInTheDocument();
		expect(await screen.findByRole('button', { name: 'Resume responses' })).toBeInTheDocument();
		expectNoRawStatus();
	});

	it('resuming sends status active through api.posts.update and the paused words go', async () => {
		const user = userEvent.setup();
		signInAs(AUTHOR);
		await renderPost(makePost({ status: 'matched' }));

		await user.click(screen.getByRole('button', { name: 'Resume responses' }));

		expect(update).toHaveBeenCalledTimes(1);
		expect(update).toHaveBeenCalledWith(POST_ID, { status: 'active' });
		await waitFor(() => expect(screen.queryByText(PAUSED)).not.toBeInTheDocument());
		expect(await screen.findByRole('button', { name: 'Pause responses' })).toBeInTheDocument();
	});

	it('a refused pause names the reason and leaves the post as it was', async () => {
		const user = userEvent.setup();
		update.mockRejectedValueOnce(new Error('this post changed while it was being edited; reload and try again'));
		signInAs(AUTHOR);
		await renderPost(makePost());

		await user.click(screen.getByRole('button', { name: 'Pause responses' }));

		expect(await screen.findByRole('alert')).toHaveTextContent('reload and try again');
		expect(screen.queryByText(PAUSED)).not.toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'Pause responses' })).toBeEnabled();
	});

	it('offers no Pause control on a post that is already closed', async () => {
		signInAs(AUTHOR);
		await renderPost(makePost({ status: 'fulfilled' }));

		expect(screen.queryByRole('button', { name: /pause|resume/i })).not.toBeInTheDocument();
	});
});

describe('post page: a paused post takes no responses', () => {
	it('shows the paused words and no respond control to a non-author', async () => {
		signInAs(STRANGER);
		await renderPost(makePost({ status: 'matched' }));

		expect(screen.getByText(PAUSED)).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
		expectNoRawStatus();
	});

	it('shows the paused words to the author without the raw status', async () => {
		signInAs(AUTHOR);
		await renderPost(makePost({ status: 'matched' }));

		expect(screen.getByText(PAUSED)).toBeInTheDocument();
		expectNoRawStatus();
	});

	it('withholds the market respond control on a paused listing too', async () => {
		signInAs(STRANGER);
		await renderPost(makePost({ kind: 'listing', status: 'matched' }));

		expect(screen.getByText(PAUSED)).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'Make an offer' })).not.toBeInTheDocument();
	});
});

describe('aid list: the respond control is offered on active posts only', () => {
	it('a paused need offers no "I can help" and says it is paused, while an active need still offers it', async () => {
		signInAs(STRANGER);
		fixture.list = [
			{ ...makePost({ id: 'paused-need', title: 'Paused need', status: 'matched' }) },
			{ ...makePost({ id: 'active-need', title: 'Active need' }) }
		];
		render(AidPage);
		await screen.findByText('Paused need');

		expect(screen.getAllByRole('button', { name: 'I can help' })).toHaveLength(1);
		expect(screen.getAllByText(PAUSED)).toHaveLength(1);
		expectNoRawStatus();
	});

	it('a paused offer offers no "Request this"', async () => {
		signInAs(STRANGER);
		fixture.list = [{ ...makePost({ kind: 'offer', title: 'Paused offer', status: 'matched' }) }];
		render(AidPage);
		await screen.findByText('Paused offer');

		expect(screen.queryByRole('button', { name: 'Request this' })).not.toBeInTheDocument();
		expect(screen.getByText(PAUSED)).toBeInTheDocument();
	});

	it('a fulfilled need offers no "I can help" either', async () => {
		signInAs(STRANGER);
		fixture.list = [{ ...makePost({ title: 'Done need', status: 'fulfilled' }) }];
		render(AidPage);
		await screen.findByText('Done need');

		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
	});
});

describe('AidCard: the respond control is offered on active posts only', () => {
	it('a paused need from another author shows the paused words and no respond control', () => {
		signInAs(STRANGER);
		render(AidCard, { props: { post: makePost({ status: 'matched' }) } });

		expect(screen.getByText(PAUSED)).toBeInTheDocument();
		expect(screen.queryByRole('button', { name: 'I can help' })).not.toBeInTheDocument();
		expectNoRawStatus();
	});

	it('an active need from another author still offers "I can help"', () => {
		signInAs(STRANGER);
		render(AidCard, { props: { post: makePost() } });

		expect(screen.getByRole('button', { name: 'I can help' })).toBeInTheDocument();
		expect(screen.queryByText(PAUSED)).not.toBeInTheDocument();
	});
});
