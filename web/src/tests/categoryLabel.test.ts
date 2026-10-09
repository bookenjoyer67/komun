import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { categoryLabel } from '$lib/api/categories';
import AidCard from '$lib/components/AidCard.svelte';
import PostPage from '../routes/p/[id]/+page.svelte';
import { serverState } from '$lib/stores/server';
import { auth } from '$lib/stores/auth';
import type { PostLike } from '$lib/api/types';

/**
 * A category reads as the human label the taxonomy carries, not the raw slug. The aid surfaces
 * rendered `{post.category}` directly, so `toys-games` showed as itself while the market grid showed
 * `Toys & Games`. F16 of the product review.
 */

const SERVER = 'https://test.komun.buzz';
const POST_ID = 'post-1';

const fixture = vi.hoisted(() => ({ post: {} as Record<string, unknown> }));

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

const makePost = (overrides: Partial<PostLike> = {}): PostLike => ({
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
});

describe('categoryLabel: the human label, the slug as a fallback', () => {
	it('prefers the label the taxonomy carries', () => {
		expect(categoryLabel({ category: 'toys-games', category_label: 'Toys & Games' })).toBe(
			'Toys & Games'
		);
	});

	it('falls back to the raw slug when no label is present', () => {
		expect(categoryLabel({ category: 'toys-games' })).toBe('toys-games');
		expect(categoryLabel({ category: 'toys-games', category_label: null })).toBe('toys-games');
	});
});

describe('the aid card renders the human category label', () => {
	it('shows the label, not the slug', () => {
		render(AidCard, {
			props: { post: makePost({ category: 'toys-games', category_label: 'Toys & Games' }) }
		});

		expect(screen.getByText('Toys & Games')).toBeInTheDocument();
		expect(screen.queryByText('toys-games')).not.toBeInTheDocument();
	});

	it('falls back to the slug when the taxonomy has no label', () => {
		render(AidCard, {
			props: { post: makePost({ category: 'toys-games', category_label: null }) }
		});

		expect(screen.getByText('toys-games')).toBeInTheDocument();
	});
});

describe('the post page renders the human category label', () => {
	it('shows the label, not the slug', async () => {
		fixture.post = { ...makePost({ category: 'baby-kids', category_label: 'Baby & Kids' }) };
		render(PostPage);
		await screen.findByRole('heading', { name: 'Need groceries' });

		expect(screen.getByText('Baby & Kids')).toBeInTheDocument();
		expect(screen.queryByText('baby-kids')).not.toBeInTheDocument();
	});
});
