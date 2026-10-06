import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import LocationBar from '$lib/components/LocationBar.svelte';
import { getActiveServer } from '$lib/stores/server';
import { clearLocation } from '$lib/stores/location';

vi.mock('$lib/stores/server', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/stores/server')>();
	return { ...actual, getActiveServer: vi.fn(() => null) };
});

type FetchMock = Mock<(input: RequestInfo | URL) => Promise<Response>>;

const ORIGIN = window.location.origin;
const hit = { lat: '30.2672', lon: '-97.7431', display_name: 'Austin, Texas, United States' };

let fetchMock: FetchMock;

function answer(respond: () => Promise<Response>) {
	fetchMock = vi.fn(async (_input: RequestInfo | URL) => respond());
	vi.stubGlobal('fetch', fetchMock);
}

function answerStatus(status: number) {
	answer(async () => new Response(JSON.stringify({ error: 'test' }), { status }));
}

async function search(onLocationSet = vi.fn()) {
	const user = userEvent.setup();
	render(LocationBar, { props: { onLocationSet } });
	await user.type(screen.getByRole('textbox'), 'Austin, TX');
	await user.click(screen.getByRole('button', { name: 'Search' }));
	return onLocationSet;
}

beforeEach(() => {
	clearLocation();
	vi.mocked(getActiveServer).mockReturnValue(ORIGIN);
	answer(async () => new Response(JSON.stringify(hit), { status: 200 }));
});

afterEach(() => {
	vi.unstubAllGlobals();
	localStorage.clear();
});

describe('location search outcomes', () => {
	it('L1: with no server selected, says so, links to the Connect page and sends nothing', async () => {
		vi.mocked(getActiveServer).mockReturnValue(null);

		await search();

		const alert = await screen.findByRole('alert');
		expect(alert).toHaveTextContent(/no server selected/i);
		expect(within(alert).getByRole('link')).toHaveAttribute('href', '/connect');
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('L2: a server on another origin is not asked, and the alert names it', async () => {
		vi.mocked(getActiveServer).mockReturnValue('https://elsewhere.example');

		await search();

		const alert = await screen.findByRole('alert');
		expect(alert).toHaveTextContent('https://elsewhere.example');
		expect(alert).toHaveTextContent(/only talks to the server it was loaded from/i);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('L3: an unreachable server is named as unreachable', async () => {
		answer(async () => {
			throw new TypeError('Failed to fetch');
		});

		await search();

		const alert = await screen.findByRole('alert');
		expect(alert).toHaveTextContent(/could not reach/i);
		expect(alert).toHaveTextContent(ORIGIN);
	});

	it('L4: a failed place lookup on the server is named as such', async () => {
		answerStatus(502);

		await search();

		expect(await screen.findByRole('alert')).toHaveTextContent(/could not look up places/i);
	});

	it.each([503, 429])('L5: a %i answer says the search is busy', async (status) => {
		answerStatus(status);

		await search();

		expect(await screen.findByRole('alert')).toHaveTextContent(/busy/i);
	});

	it('L6: a place the server does not know is reported as not found', async () => {
		answerStatus(404);

		await search();

		expect(await screen.findByText('Location not found. Try a different search.')).toBeInTheDocument();
	});

	it('L6b: the not-found message is announced as an alert', async () => {
		answerStatus(404);

		await search();

		expect(await screen.findByRole('alert')).toHaveTextContent('Location not found. Try a different search.');
	});

	it('L7: a found place is set and raises no alert', async () => {
		const onLocationSet = await search();

		await waitFor(() => expect(onLocationSet).toHaveBeenCalled());
		expect(screen.queryByRole('alert')).not.toBeInTheDocument();
	});
});
