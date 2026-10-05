import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
// Leaflet has no TypeScript declarations here, as in LocationMap.svelte.
// @ts-ignore
import * as L from 'leaflet';
import AidCard from '$lib/components/AidCard.svelte';
import { auth } from '$lib/stores/auth';
import type { PostLike } from '$lib/api/types';

vi.mock('$lib/stores/server', () => ({
	getActiveServer: vi.fn(() => 'https://test.komun.buzz'),
	setActiveServer: vi.fn(),
	servers: { subscribe: vi.fn() }
}));

vi.mock('$lib/api/discovery', () => ({}));

/**
 * A6 flattened the model: a post belongs to a server, not to a community inside one. The
 * fixture is typed as the real `PostLike` now — the twelve type errors this file used to carry
 * all came from an untyped literal being passed to a prop that wanted a `kind` union, and a
 * fixture that cannot drift from the component's contract cannot reproduce them.
 */
const makePost = (overrides: Partial<PostLike> = {}): PostLike => ({
	id: 'post-1',
	author_id: 'author-1',
	kind: 'need',
	category: 'food',
	title: 'Need groceries',
	body: 'Can someone help with groceries this week?',
	urgency: 'high',
	status: 'active',
	created_at: new Date(Date.now() - 3600000).toISOString(),
	server_name: 'stl.komun.buzz',
	server_url: 'https://stl.komun.buzz',
	server_location: 'St. Louis, MO',
	...overrides
});

describe('AidCard', () => {
	beforeEach(() => {
		auth.set({ keypair: null, servers: {} });
	});

	it('renders post title', () => {
		render(AidCard, { props: { post: makePost() } });
		expect(screen.getByText('Need groceries')).toBeInTheDocument();
	});

	it('renders post body', () => {
		render(AidCard, { props: { post: makePost() } });
		expect(screen.getByText('Can someone help with groceries this week?')).toBeInTheDocument();
	});

	it('renders kind label', () => {
		render(AidCard, { props: { post: makePost({ kind: 'need' }) } });
		expect(screen.getByText('Need')).toBeInTheDocument();
	});

	it('shows Offer kind label', () => {
		render(AidCard, { props: { post: makePost({ kind: 'offer' }) } });
		expect(screen.getByText('Offer')).toBeInTheDocument();
	});

	it('shows Resource kind label', () => {
		render(AidCard, { props: { post: makePost({ kind: 'resource' }) } });
		expect(screen.getByText('Resource')).toBeInTheDocument();
	});

	// Replaces `shows community name and server`. Its subject — the community a post belonged
	// to — no longer exists; the origin a federated feed still has to show is the server.
	it('shows the originating server, not a community', () => {
		render(AidCard, { props: { post: makePost() } });
		expect(screen.getByText('stl.komun.buzz')).toBeInTheDocument();
		expect(screen.queryByText('Mutual Aid STL')).not.toBeInTheDocument();
	});

	// The permalink is flat now: `/p/{id}`, with no community segment to resolve first.
	it('links to the flat post permalink', () => {
		render(AidCard, { props: { post: makePost({ id: 'post-42' }) } });
		expect(screen.getByTitle('Open this post')).toHaveAttribute('href', '/p/post-42');
	});

	it('shows "I can help" button for need post by other author', () => {
		render(AidCard, { props: { post: makePost({ kind: 'need', author_id: 'other-author' }) } });
		expect(screen.getByText('I can help')).toBeInTheDocument();
	});

	it('shows "Request this" button for offer post by other author', () => {
		render(AidCard, { props: { post: makePost({ kind: 'offer', author_id: 'other-author' }) } });
		expect(screen.getByText('Request this')).toBeInTheDocument();
	});

	it('shows "Your post" for own posts', () => {
		auth.set({
			keypair: null,
			servers: {
				'https://test.komun.buzz': { token: 'jwt', userId: 'my-id', displayName: 'Me', role: 'member' }
			}
		});
		render(AidCard, {
			props: {
				post: makePost({
					author_id: 'my-id',
					server_url: 'https://test.komun.buzz'
				})
			}
		});
		expect(screen.getByText('Your post')).toBeInTheDocument();
	});

	it('does not show respond button for own posts', () => {
		auth.set({
			keypair: null,
			servers: {
				'https://test.komun.buzz': { token: 'jwt', userId: 'my-id', displayName: 'Me', role: 'member' }
			}
		});
		render(AidCard, {
			props: {
				post: makePost({
					author_id: 'my-id',
					server_url: 'https://test.komun.buzz'
				})
			}
		});
		expect(screen.queryByText('I can help')).not.toBeInTheDocument();
		expect(screen.queryByText('Request this')).not.toBeInTheDocument();
	});

	it('opens respond modal on button click', async () => {
		const user = userEvent.setup();
		render(AidCard, { props: { post: makePost({ kind: 'need', author_id: 'other' }) } });
		await user.click(screen.getByText('I can help'));
		// The modal no longer collects a display name: A2a replaced the anonymous device
		// identity with a session, so an unauthenticated visitor is sent to sign up instead.
		expect(screen.getByText('Offer help')).toBeInTheDocument();
		expect(screen.getByText('Create an account')).toBeInTheDocument();
	});

	it('hides body when not provided', () => {
		render(AidCard, { props: { post: makePost({ body: undefined }) } });
		expect(screen.queryByText('Can someone help with groceries this week?')).not.toBeInTheDocument();
	});
});

/**
 * The card may show a post from another server that still sends exact coordinates, so the
 * map must be fed the coarse values whatever arrives. The Leaflet spies read what the map was
 * actually given, not what the card meant to give it.
 */
describe('AidCard map', () => {
	let setView: ReturnType<typeof vi.spyOn>;
	let marker: ReturnType<typeof vi.spyOn>;

	beforeEach(() => {
		auth.set({ keypair: null, servers: {} });
		setView = vi.spyOn(L.Map.prototype, 'setView');
		marker = vi.spyOn(L.Marker.prototype, 'initialize');
	});

	afterEach(() => {
		setView.mockRestore();
		marker.mockRestore();
	});

	const exact = { location_lat: 37.80443, location_lon: -122.27121 };
	const coarse = { lat: 37.8, lng: -122.3 };

	function latLngs(spy: ReturnType<typeof vi.spyOn>) {
		return spy.mock.calls.map((args: unknown[]) => {
			const point = L.latLng(args[0]);
			return { lat: point.lat, lng: point.lng };
		});
	}

	async function openMap() {
		const user = userEvent.setup();
		render(AidCard, { props: { post: makePost({ author_id: 'other', ...exact }) } });
		await user.click(screen.getByTitle('View on map'));
	}

	it('embeds no third-party map frame', async () => {
		await openMap();
		expect(document.querySelector('iframe')).toBeNull();
		expect(document.body.innerHTML).not.toContain('openstreetmap.org/export');
	});

	it('centres and pins the in-app map on the coarse coordinates', async () => {
		await openMap();

		await waitFor(() => expect(setView).toHaveBeenCalled());
		await waitFor(() => expect(marker).toHaveBeenCalled());
		for (const centre of latLngs(setView)) {
			expect(centre).toEqual(coarse);
		}
		for (const pin of latLngs(marker)) {
			expect(pin).toEqual(coarse);
		}
	});
});
