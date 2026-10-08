import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { goto } from '$app/navigation';
import NotificationsPage from '../routes/notifications/+page.svelte';

/**
 * A notification's link is whatever string the active server returned, so only a same-origin
 * absolute path may reach goto. Every refused input here is inert (a no-op script URL, a
 * plain-text data URL), enough to prove the check without a working payload in a public
 * repository.
 */
vi.mock('$lib/stores/server', () => ({
	isConnected: () => true,
	getActiveServer: () => 'https://api.test'
}));

vi.mock('$lib/stores/auth', () => ({
	isAuthenticated: () => true,
	getToken: () => 'test-token'
}));

const SERVER = 'https://api.test';
const NOTIF_ID = 'notif-r8';
const TITLE = 'Notification R8';

function installServer(notification: Record<string, unknown>) {
	const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
		const url = String(input);
		if (url === `${SERVER}/api/me/notifications` && !init?.method) {
			return new Response(JSON.stringify([notification]), { status: 200 });
		}
		return new Response('{}', { status: 200 });
	});
	vi.stubGlobal('fetch', fetchMock);
	return fetchMock;
}

async function clickNotification(link: string | undefined, read = true) {
	const fetchMock = installServer({
		id: NOTIF_ID,
		kind: 'message',
		title: TITLE,
		link,
		read,
		created_at: new Date().toISOString()
	});
	const user = userEvent.setup();
	render(NotificationsPage);
	const item = (await screen.findByText(TITLE)).closest('button');
	expect(item).toHaveClass('notif-item');
	await user.click(item as HTMLElement);
	return fetchMock;
}

beforeEach(() => {
	vi.mocked(goto).mockClear();
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('notification links (R8)', () => {
	it('follows a same-origin absolute path', async () => {
		const link = '/messages/0190a0b0-0000-7000-8000-000000000001';

		await clickNotification(link);

		expect(goto).toHaveBeenCalledTimes(1);
		expect(goto).toHaveBeenCalledWith(link);
	});

	it.each([
		['a script URL', 'javascript:void(0)'],
		['an upper-case script URL', 'JAVASCRIPT:void(0)'],
		['a script URL behind leading whitespace', '  javascript:void(0)'],
		['an absolute URL on another origin', 'https://other.example/messages/1'],
		['a protocol-relative link', '//other.example/messages/1'],
		['a slash-backslash link', '/\\other.example/messages/1'],
		['a data URL', 'data:text/plain,hello'],
		['a relative path with no leading slash', 'messages/1'],
		['an empty string', '']
	])('does not navigate for %s', async (_name, link) => {
		await clickNotification(link);

		expect(goto).not.toHaveBeenCalled();
	});

	it('does not navigate when the notification has no link', async () => {
		await clickNotification(undefined);

		expect(goto).not.toHaveBeenCalled();
	});

	it('still marks an unread notification read when its link is refused', async () => {
		const fetchMock = await clickNotification('javascript:void(0)', false);

		expect(fetchMock).toHaveBeenCalledWith(
			`${SERVER}/api/me/notifications/${NOTIF_ID}/read`,
			expect.objectContaining({ method: 'PATCH' })
		);
	});
});
