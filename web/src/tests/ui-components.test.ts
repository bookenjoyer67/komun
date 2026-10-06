import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import EmptyState from '$lib/components/ui/EmptyState.svelte';

/** A snippet that renders fixed markup, for exercising slot-rendering components. */
function textSnippet(html: string) {
	return createRawSnippet(() => ({ render: () => html }));
}

describe('EmptyState', () => {
	it('renders the title', () => {
		render(EmptyState, { props: { title: 'No conversations yet.' } });
		expect(screen.getByText('No conversations yet.')).toBeTruthy();
	});

	it('renders the description when given', () => {
		render(EmptyState, {
			props: { title: 'No offers yet.', description: 'When someone offers, it appears here.' }
		});
		expect(screen.getByText('When someone offers, it appears here.')).toBeTruthy();
	});

	it('omits the description when not given', () => {
		const { container } = render(EmptyState, { props: { title: 'Nothing here.' } });
		expect(container.querySelector('.description')).toBeNull();
	});

	it('omits the action container when there is no call to action', () => {
		const { container } = render(EmptyState, { props: { title: 'Nothing here.' } });
		expect(container.querySelector('.action')).toBeNull();
	});

	it('renders a call to action when given one', () => {
		render(EmptyState, {
			props: { title: 'No listings yet.', children: textSnippet('<a href="/new">Post one</a>') }
		});
		expect(screen.getByRole('link', { name: 'Post one' })).toBeTruthy();
	});
});
