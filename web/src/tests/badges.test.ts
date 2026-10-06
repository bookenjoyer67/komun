import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import UserPage from '../routes/users/[id]/+page.svelte';

/**
 * D3: the server decides which badges a user has earned and sends only those. Each fixture below
 * is the server's answer for that endorsement count; the page renders it and never re-derives it.
 */
function renderProfile(endorsement_count: number, badges: unknown) {
	const data = {
		profile: {
			id: 'user-badges',
			display_name: 'Test Profile',
			role: 'user',
			avatar_url: null,
			bio: null,
			rating_count: 0,
			rating_avg: null,
			post_count: 0,
			verified_post_count: 0,
			endorsement_count,
			badges,
			joined_at: null,
			last_seen: null,
			public_key: 'fake-test-public-key-not-a-real-key-value',
			profile_json: null
		},
		isOwnProfile: false,
		endorsements: { count: endorsement_count, endorsements: [] },
		hasEndorsed: false,
		error: null
	};
	// The page's data type is generated from its loader's union of outcomes; this fixture is the
	// loaded-profile outcome only.
	return render(UserPage, { props: { data } as never });
}

function badgeLabels(container: HTMLElement): (string | undefined)[] {
	return Array.from(container.querySelectorAll('.reciprocity-badge')).map((item) =>
		item.textContent?.trim()
	);
}

const RELIABLE = { code: 'reliable', label: 'Reliable' };

describe('profile reciprocity badges (D3)', () => {
	it('shows Reliable for three endorsements', () => {
		const { container } = renderProfile(3, [RELIABLE]);

		expect(container.querySelector('.reciprocity-badges')).not.toBeNull();
		expect(badgeLabels(container)).toEqual(['Reliable']);
	});

	it('shows no badge for two endorsements', () => {
		const { container } = renderProfile(2, []);

		expect(container.querySelector('.reciprocity-badges')).toBeNull();
		expect(container.textContent).not.toContain('Reliable');
	});

	it('shows every earned badge as a label and never as a number', () => {
		const { container } = renderProfile(3, [
			RELIABLE,
			{ code: 'regular_giver', label: 'Regular giver' }
		]);

		const labels = badgeLabels(container);
		expect(labels).toEqual(['Reliable', 'Regular giver']);
		for (const label of labels) {
			expect(label).not.toMatch(/\d/);
		}
	});

	it('renders nothing for a missing or malformed badges field', () => {
		for (const badges of [undefined, null, 'reliable', [{ code: 'reliable' }], [null]]) {
			const { container, unmount } = renderProfile(3, badges);
			expect(container.querySelector('.reciprocity-badges')).toBeNull();
			unmount();
		}
	});
});
