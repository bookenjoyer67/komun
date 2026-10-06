import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';

const discovery = vi.hoisted(() => ({ real: false }));

vi.mock('$lib/api/discovery', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/api/discovery')>();
	return {
		...actual,
		discoverNearbyServers: () => (discovery.real ? actual.discoverNearbyServers() : Promise.resolve([]))
	};
});

const ORIGIN = window.location.origin;
const hit = { lat: '30.2672', lon: '-97.7431', display_name: 'Austin, Texas, United States' };
const node = { name: 'Test Node', description: 'd', version: '0.1.0', listed: false };

/** Answers only the listed URLs (query string ignored); anything else fails as an unreachable server does. */
function installServer(routes: Record<string, unknown>) {
	const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
		const key = String(input).split('?')[0];
		if (key in routes) return new Response(JSON.stringify(routes[key]), { status: 200 });
		throw new TypeError('Failed to fetch');
	});
	vi.stubGlobal('fetch', fetchMock);
	return fetchMock;
}

function originOf(input: RequestInfo | URL): string {
	try {
		return new URL(String(input)).origin;
	} catch {
		return String(input);
	}
}

async function searchFromHome(place: string) {
	const { default: Home } = await import('../routes/+page.svelte');
	const user = userEvent.setup();
	render(Home);
	await user.type(screen.getByRole('textbox'), place);
	await user.click(screen.getByRole('button', { name: 'Search' }));
}

afterEach(() => {
	discovery.real = false;
	vi.unstubAllGlobals();
	localStorage.clear();
});

describe('first visit', () => {
	// The stores read storage once, on first import. F1 must stay first so that import happens
	// with nothing stored, as on a real first visit.
	it('F1: a first-time visitor who searches a city gets it, asking only the origin that served the app', async () => {
		localStorage.clear();
		const fetchMock = installServer({ [`${ORIGIN}/api/geocode`]: hit });

		await searchFromHome('Austin, TX');

		expect(await screen.findByText('Austin, Texas')).toBeInTheDocument();
		expect(screen.queryByText(/Location not found/)).not.toBeInTheDocument();
		expect(fetchMock).toHaveBeenCalled();
		for (const [input] of fetchMock.mock.calls) {
			expect(originOf(input)).toBe(ORIGIN);
		}
	});

	it('F2: after a search on the serving origin, the feed counts that server instead of reporting none nearby', async () => {
		const { serverState } = await import('$lib/stores/server');
		const { clearLocation } = await import('$lib/stores/location');
		clearLocation();
		serverState.set({ active: ORIGIN, known: [] });
		discovery.real = true;
		installServer({
			[`${ORIGIN}/api/geocode`]: hit,
			[`${ORIGIN}/api/node`]: node,
			[`${ORIGIN}/api/directory`]: [],
			[`${ORIGIN}/api/posts`]: []
		});

		await searchFromHome('Austin, TX');

		expect(await screen.findByText(/Found 1 server nearby/)).toBeInTheDocument();
		expect(screen.queryByText(/No servers found nearby/)).not.toBeInTheDocument();
	});
});
