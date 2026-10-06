import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { tick } from 'svelte';

vi.mock('$lib/api/client', () => ({
	api: { posts: { list: vi.fn(async () => []) } }
}));

import { api } from '$lib/api/client';
import { serverState } from '$lib/stores/server';
import { clearLocation, location as savedLocation } from '$lib/stores/location';
import SearchPage from '../routes/search/+page.svelte';
import {
	DEFAULT_RADIUS_KM,
	distanceLabel,
	listMarketPosts,
	nextRadius,
	parseRadiusChoice,
	RADIUS_CHOICES,
	RADIUS_PRESETS_KM,
	radiusParams,
	type MarketPost
} from '$lib/api/market';

const list = vi.mocked(api.posts.list);

// An exact point; what leaves the browser must be its 0.1-degree cell.
const HOME = { lat: 37.80443, lon: -122.27121 };

const post = (id: string, kind: string, distance_km: number | null, created_at: string): MarketPost =>
	({ id, kind, category: 'furniture', title: id, created_at, distance_km }) as MarketPost;

// A negative assertion is only meaningful once queued effects and the fetches they start have run.
async function settle() {
	await tick();
	await new Promise((resolve) => setTimeout(resolve, 0));
	await tick();
}

// A false empty result that is rendered and then replaced is still a wrong answer, so every text the page renders is kept.
function watchText(root: HTMLElement): () => string {
	const seen: string[] = [];
	const collect = (records: MutationRecord[]) => {
		for (const r of records) {
			if (r.type === 'characterData') seen.push(r.target.textContent ?? '');
			r.addedNodes.forEach((n) => seen.push(n.textContent ?? ''));
		}
	};
	const observer = new MutationObserver(collect);
	observer.observe(root, { childList: true, subtree: true, characterData: true });
	return () => {
		collect(observer.takeRecords());
		observer.disconnect();
		return seen.join('\n');
	};
}

const usersQueries = () =>
	vi
		.mocked(globalThis.fetch)
		.mock.calls.map(([url]) => String(url))
		.filter((url) => url.includes('/api/search/users'))
		.map((url) => decodeURIComponent(url.split('?q=')[1] ?? ''));

beforeEach(() => {
	list.mockReset();
	list.mockResolvedValue([]);
});

describe('radius presets', () => {
	it('offers 15, 25, 50 and 100 km plus any, defaulting to 25 km', () => {
		expect([...RADIUS_PRESETS_KM]).toEqual([15, 25, 50, 100]);
		expect(DEFAULT_RADIUS_KM).toBe(25);
		expect(RADIUS_CHOICES.map((c) => c.value)).toEqual([15, 25, 50, 100, 'any']);
	});

	it('widens one preset at a time, then to any, then no further', () => {
		expect(nextRadius(15)).toBe(25);
		expect(nextRadius(25)).toBe(50);
		expect(nextRadius(50)).toBe(100);
		expect(nextRadius(100)).toBe('any');
		expect(nextRadius('any')).toBeNull();
	});

	it('reads an unknown select value as the default, never as a radius the server would refuse', () => {
		expect(parseRadiusChoice('50')).toBe(50);
		expect(parseRadiusChoice('any')).toBe('any');
		expect(parseRadiusChoice('7')).toBe(DEFAULT_RADIUS_KM);
		expect(parseRadiusChoice('')).toBe(DEFAULT_RADIUS_KM);
	});
});

describe('distance label', () => {
	it('is always approximate and whole', () => {
		expect(distanceLabel(12.4)).toBe('~12 km');
		expect(distanceLabel(24)).toBe('~24 km');
		expect(distanceLabel(0)).toBe('~0 km');
	});

	it('is empty when there is no distance', () => {
		expect(distanceLabel(null)).toBe('');
		expect(distanceLabel(undefined)).toBe('');
		expect(distanceLabel(Number.NaN)).toBe('');
	});
});

describe('radius params', () => {
	it('sends the coarse cell of the centre, never the exact point', () => {
		expect(radiusParams(HOME)).toEqual({ near_lat: '37.8', near_lon: '-122.3', radius_km: '25' });
	});

	it('sends a centre with no radius for "any"', () => {
		expect(radiusParams(HOME, 'any')).toEqual({ near_lat: '37.8', near_lon: '-122.3' });
	});

	it('sends nothing without a whole centre, because the server refuses half of one', () => {
		expect(radiusParams(null, 50)).toEqual({});
		expect(radiusParams({ lat: 37.8, lon: null }, 50)).toEqual({});
		expect(radiusParams({ lat: null, lon: -122.3 }, 50)).toEqual({});
	});
});

describe('listMarketPosts with a centre', () => {
	it('asks /api/posts for the coarse centre and the chosen radius', async () => {
		await listMarketPosts({ kind: 'listing', radius_km: 50 }, HOME);
		expect(list).toHaveBeenCalledWith({
			kind: 'listing',
			near_lat: '37.8',
			near_lon: '-122.3',
			radius_km: '50'
		});
	});

	it('drops a distance order when there is no centre to measure from', async () => {
		await listMarketPosts({ kind: 'want', sort: 'distance', radius_km: 15 }, null);
		expect(list).toHaveBeenCalledWith({ kind: 'want' });
	});

	it('merges both kinds nearest first, with unlocated posts last', async () => {
		list.mockImplementation(async (params?: Record<string, string>) =>
			params?.kind === 'listing'
				? [post('far', 'listing', 40, '2026-10-01T00:00:00Z'), post('nowhere', 'listing', null, '2026-10-05T00:00:00Z')]
				: [post('near', 'want', 3, '2026-09-01T00:00:00Z')]
		);

		const merged = await listMarketPosts({ sort: 'distance' }, HOME);
		expect(merged.map((p) => p.id)).toEqual(['near', 'far', 'nowhere']);
		for (const call of list.mock.calls) {
			expect(call[0]).toMatchObject({ sort: 'distance', near_lat: '37.8', near_lon: '-122.3' });
		}
	});
});

describe('search page radius and centre', () => {
	beforeEach(() => {
		serverState.set({ active: 'https://test.komun.buzz', known: [] });
		savedLocation.set({ name: 'Home', lat: HOME.lat, lon: HOME.lon });
		// The users half of the page reads /api/search/users through fetch, outside the mocked client.
		vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => [] })));
	});

	afterEach(() => {
		vi.unstubAllGlobals();
		clearLocation();
		serverState.set({ active: null, known: [] });
	});

	it('never searches an empty query, and never shows "Searching..." for one', async () => {
		const user = userEvent.setup();
		render(SearchPage, { props: { data: { q: '' } } });

		await user.selectOptions(screen.getByRole('combobox'), '50');
		await user.click(screen.getByRole('button', { name: 'Widen the area' }));
		savedLocation.set({ name: 'Elsewhere', lat: 51.5072, lon: -0.1276 });
		await settle();

		expect(list).not.toHaveBeenCalled();
		expect(screen.queryByText('Searching...')).not.toBeInTheDocument();
		expect(screen.getByText('Type a word to search posts and users.')).toBeInTheDocument();
	});

	it('treats typed but unsubmitted text as no search at all', async () => {
		const user = userEvent.setup();
		render(SearchPage, { props: { data: { q: '' } } });
		await settle();

		await user.type(screen.getByRole('searchbox'), 'ladder');
		await settle();
		expect(screen.getByText('Type a word to search posts and users.')).toBeInTheDocument();
		expect(screen.queryByText(/No posts found/)).not.toBeInTheDocument();

		await user.selectOptions(screen.getByRole('combobox'), '50');
		await settle();
		expect(list).not.toHaveBeenCalled();

		await user.click(screen.getByRole('button', { name: 'Widen the area' }));
		await settle();
		expect(list).not.toHaveBeenCalled();

		savedLocation.set({ name: 'Elsewhere', lat: 51.5072, lon: -0.1276 });
		await settle();
		expect(list).not.toHaveBeenCalled();
		expect(screen.getByText('Type a word to search posts and users.')).toBeInTheDocument();
		expect(screen.queryByText(/No posts found/)).not.toBeInTheDocument();

		await user.click(screen.getByRole('button', { name: /^Users/ }));
		await settle();
		expect(screen.getByText('Type a word to search posts and users.')).toBeInTheDocument();
		expect(screen.queryByText(/No users found/)).not.toBeInTheDocument();
		expect(screen.getByRole('searchbox')).toHaveValue('ladder');
	});

	it('searches again from the new coarse centre when the saved location moves', async () => {
		render(SearchPage, { props: { data: { q: 'ladder' } } });
		await waitFor(() => expect(list).toHaveBeenCalledTimes(1));
		expect(list).toHaveBeenLastCalledWith({
			q: 'ladder',
			near_lat: '37.8',
			near_lon: '-122.3',
			radius_km: '25'
		});

		savedLocation.set({ name: 'Elsewhere', lat: 51.5072, lon: -0.1276 });

		await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
		expect(list).toHaveBeenLastCalledWith({
			q: 'ladder',
			near_lat: '51.5',
			near_lon: '-0.1',
			radius_km: '25'
		});
	});

	// SvelteKit reuses this component for `goto('/search?q=…')`, so a submit arrives as a new `data` on the same instance.
	it('searches a query submitted from an empty page, showing "Searching..." until the answer arrives', async () => {
		let answer: (posts: MarketPost[]) => void = () => {};
		list.mockImplementation(() => new Promise<MarketPost[]>((resolve) => (answer = resolve)));
		const user = userEvent.setup();
		const { container, rerender } = render(SearchPage, { props: { data: { q: '' } } });
		await settle();
		expect(list).not.toHaveBeenCalled();

		const seen = watchText(container);
		const box = screen.getByRole('searchbox');
		await user.type(box, 'ladder');
		await rerender({ data: { q: 'ladder' } });
		await settle();

		expect(screen.getByRole('searchbox')).toBe(box);
		expect(screen.queryByText(/No posts found/)).not.toBeInTheDocument();
		expect(screen.getByText('Searching...')).toBeInTheDocument();
		expect(list).toHaveBeenCalledTimes(1);
		expect(list).toHaveBeenLastCalledWith({
			q: 'ladder',
			near_lat: '37.8',
			near_lon: '-122.3',
			radius_km: '25'
		});
		expect(usersQueries()).toEqual(['ladder']);

		answer([post('ladder-post', 'listing', 3, '2026-10-01T00:00:00Z')]);
		await waitFor(() => expect(screen.getByText('ladder-post')).toBeInTheDocument());
		expect(screen.queryByText('Searching...')).not.toBeInTheDocument();
		expect(seen()).not.toMatch(/No posts found/);
	});

	it('searches again when the submitted query changes from one word to another', async () => {
		const { rerender } = render(SearchPage, { props: { data: { q: 'ladder' } } });
		await waitFor(() => expect(list).toHaveBeenCalledTimes(1));
		await settle();
		expect(list).toHaveBeenCalledTimes(1);
		expect(usersQueries()).toEqual(['ladder']);

		const box = screen.getByRole('searchbox');
		await rerender({ data: { q: 'drill' } });

		await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
		expect(screen.getByRole('searchbox')).toBe(box);
		expect(list).toHaveBeenLastCalledWith({
			q: 'drill',
			near_lat: '37.8',
			near_lon: '-122.3',
			radius_km: '25'
		});
		await settle();
		expect(list).toHaveBeenCalledTimes(2);
		expect(usersQueries()).toEqual(['ladder', 'drill']);
		expect(screen.getByText('No posts found for "drill" within 25 km.')).toBeInTheDocument();
	});
});
