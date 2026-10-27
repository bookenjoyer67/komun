import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { serverState } from '$lib/stores/server';
import { auth } from '$lib/stores/auth';
import AidPage from '../routes/aid/+page.svelte';

/**
 * An aid card must be openable from the list, the way a market card already is: `/aid` rendered
 * legacy cards with no permalink, so a post could not be reached from there.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-42';

const fixture = vi.hoisted(() => ({
	list: [] as Record<string, unknown>[]
}));

vi.mock('$lib/api/client', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/client')>();
	return {
		...actual,
		api: {
			...actual.api,
			posts: { ...actual.api.posts, list: async () => fixture.list.map((p) => ({ ...p })) }
		}
	};
});

const makePost = (overrides: Record<string, unknown> = {}) => ({
	id: POST_ID,
	author_id: 'author-1',
	kind: 'need',
	category: 'food',
	title: 'Need groceries',
	status: 'active',
	created_at: new Date().toISOString(),
	...overrides
});

beforeEach(() => {
	vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
	serverState.set({ active: SERVER, known: [] });
	auth.set({ keypair: null, servers: {} });
});

afterEach(() => {
	vi.unstubAllGlobals();
	auth.set({ keypair: null, servers: {} });
	serverState.set({ active: null, known: [] });
	fixture.list = [];
});

describe('/aid: every card links to its post permalink', () => {
	it('renders an anchor to /p/<id> for the listed post', async () => {
		fixture.list = [makePost({ id: POST_ID, title: 'Aid permalink post' })];
		render(AidPage);
		await screen.findByText('Aid permalink post');

		expect(screen.getByTitle('Open this post')).toHaveAttribute('href', `/p/${POST_ID}`);
	});
});
