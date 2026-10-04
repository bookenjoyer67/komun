import { describe, it, expect } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import LocationMap, { popupContent } from '$lib/components/LocationMap.svelte';

/**
 * R15 F1: a marker label is untrusted listing text, so markup in it must stay text in the popup.
 * The label below is inert markup (an emphasis element), enough to tell parsed from unparsed.
 */
const MARKUP_LABEL = '<em>Corner shop</em>';

describe('LocationMap popup content (R15 F1)', () => {
	it('carries a markup label as text, with no parsed element', () => {
		const node = popupContent(MARKUP_LABEL);

		expect(node).toBeInstanceOf(HTMLElement);
		expect(node.childElementCount).toBe(0);
		expect(node.querySelector('em')).toBeNull();
		expect(node.textContent).toBe(MARKUP_LABEL);
	});

	it('carries an empty label as an empty element', () => {
		const node = popupContent('');

		expect(node.childElementCount).toBe(0);
		expect(node.textContent).toBe('');
	});

	it('shows the label as literal text in the opened popup', async () => {
		const { container } = render(LocationMap, {
			props: { lat: 0, lon: 0, markers: [{ lat: 1, lon: 2, label: MARKUP_LABEL }] }
		});

		const marker = container.querySelector('.komun-map-marker');
		expect(marker).not.toBeNull();
		await fireEvent.click(marker as Element);

		const content = await waitFor(() => {
			const found = container.querySelector('.leaflet-popup-content');
			expect(found).not.toBeNull();
			return found as Element;
		});
		expect(content.querySelector('em')).toBeNull();
		expect(content.textContent).toContain(MARKUP_LABEL);
	});
});
