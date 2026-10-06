import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import UserPage from '../routes/users/[id]/+page.svelte';

/**
 * R-2: the server withholds rating_avg (null) until rating_count reaches its publish threshold, and
 * keeps rating_count visible. The page must never print the withheld value.
 */
function renderWithRating(rating_avg: number | null, rating_count: number) {
	const data = {
		profile: {
			id: 'user-rating',
			display_name: 'Test Profile',
			role: 'user',
			avatar_url: null,
			bio: null,
			rating_count,
			rating_avg,
			post_count: 0,
			verified_post_count: 0,
			joined_at: null,
			last_seen: null,
			public_key: 'fake-test-public-key-not-a-real-key-value',
			profile_json: null
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

describe('profile rating publication (R-2)', () => {
	it('shows a withheld rating as not yet rated, keeping the count', () => {
		const { container } = renderWithRating(null, 3);

		const badge = container.querySelector('.rating-badge');
		expect(badge).not.toBeNull();
		expect(container.textContent).not.toContain('null');
		expect(badge?.getAttribute('aria-label') ?? '').not.toContain('null');
		expect(container.querySelector('.rating-stars')).toBeNull();
		expect(container.querySelector('.rating-value')).toBeNull();
		expect(badge?.textContent).toContain('Not yet rated publicly');
		expect(badge?.textContent).toMatch(/3\s+reviews/);
	});

	it('shows the average once it is published', () => {
		const { container } = renderWithRating(4.2, 5);

		const badge = container.querySelector('.rating-badge');
		expect(container.querySelector('.rating-stars')?.textContent?.trim()).toBe('★★★★☆');
		expect(container.querySelector('.rating-value')?.textContent).toBe('4.2');
		expect(badge?.getAttribute('aria-label')).toBe('Rated 4.2 out of 5 from 5 reviews');
		expect(badge?.textContent).toMatch(/5\s+reviews/);
		expect(badge?.textContent).not.toContain('Not yet rated publicly');
	});

	it('shows no reviews when there are none', () => {
		const { container } = renderWithRating(null, 0);

		const badge = container.querySelector('.rating-badge');
		expect(badge?.textContent).toContain('No reviews yet');
		expect(container.textContent).not.toContain('null');
		expect(container.querySelector('.rating-stars')).toBeNull();
	});
});
