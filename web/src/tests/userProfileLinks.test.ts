import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import UserPage from '../routes/users/[id]/+page.svelte';

/**
 * R15 F3 on the profile page, the only place profile links render. The rejected links are inert
 * (a no-op script URL, a plain-text data URL): the test proves they never reach an href, without
 * carrying a working payload.
 */
function renderWithLinks(links: unknown) {
	const data = {
		profile: {
			id: 'user-r15',
			display_name: 'Test Profile',
			role: 'user',
			avatar_url: null,
			bio: null,
			rating_count: 0,
			rating_avg: 0,
			post_count: 0,
			verified_post_count: 0,
			joined_at: null,
			last_seen: null,
			public_key: 'fake-test-public-key-not-a-real-key-value',
			profile_json: { links }
		},
		isOwnProfile: false,
		endorsements: null,
		hasEndorsed: false,
		error: null
	};
	// The page's data type is generated from its loader's union of outcomes; this fixture is the
	// loaded-profile outcome only.
	return render(UserPage, { props: { data } as never });
}

function hrefs(): (string | null)[] {
	return Array.from(document.querySelectorAll('a')).map((a) => a.getAttribute('href'));
}

describe('profile links (R15 F3)', () => {
	it('renders only the http(s) link and drops the others', () => {
		const { container } = renderWithLinks([
			{ label: 'Site', url: 'https://example.org/' },
			{ label: 'Inert', url: 'javascript:void(0)' },
			{ label: 'Plain data', url: 'data:text/plain,hello' }
		]);

		const links = container.querySelectorAll('a.profile-link');
		expect(links).toHaveLength(1);
		expect(links[0].getAttribute('href')).toBe('https://example.org/');
		expect(links[0].textContent).toBe('Site');
		for (const href of hrefs()) {
			expect(href?.trim().toLowerCase().startsWith('javascript:')).not.toBe(true);
			expect(href?.trim().toLowerCase().startsWith('data:')).not.toBe(true);
		}
		expect(container.textContent).not.toContain('Inert');
		expect(container.textContent).not.toContain('Plain data');
	});

	it('renders no links section when every link is rejected', () => {
		const { container } = renderWithLinks([{ label: 'Inert', url: 'javascript:void(0)' }]);

		expect(container.querySelector('.links-section')).toBeNull();
		expect(container.querySelectorAll('a.profile-link')).toHaveLength(0);
	});

	it('ignores a links value that is not an array', () => {
		const { container } = renderWithLinks({ url: 'https://example.org/' });

		expect(container.querySelector('.links-section')).toBeNull();
	});
});
