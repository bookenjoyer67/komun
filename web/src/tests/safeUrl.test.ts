import { describe, it, expect } from 'vitest';
import { safeHttpUrl } from '$lib/safeUrl';

/**
 * R15 F3: only an absolute http(s) URL may become a profile-link href. Every rejected input here is
 * inert (a no-op script URL, a plain-text data URL), enough to prove the scheme check without a
 * working payload in a public repository.
 */
describe('safeHttpUrl (R15 F3)', () => {
	it('accepts absolute http and https URLs and returns the parsed href', () => {
		expect(safeHttpUrl('https://example.org/me')).toBe('https://example.org/me');
		expect(safeHttpUrl('http://example.org')).toBe('http://example.org/');
		expect(safeHttpUrl('  https://example.org/me  ')).toBe('https://example.org/me');
	});

	it.each([
		['a script URL', 'javascript:void(0)'],
		['an upper-case script URL', 'JAVASCRIPT:void(0)'],
		['a script URL behind leading whitespace', '  javascript:void(0)'],
		['a data URL', 'data:text/plain,hello'],
		['an ftp URL', 'ftp://example.org'],
		['a mailto URL', 'mailto:someone@example.org'],
		['a relative path', '/users/1'],
		['an empty string', '']
	])('rejects %s', (_name, input) => {
		expect(safeHttpUrl(input)).toBeNull();
	});

	it('rejects non-string input', () => {
		expect(safeHttpUrl(undefined)).toBeNull();
		expect(safeHttpUrl(null)).toBeNull();
		expect(safeHttpUrl(42)).toBeNull();
		expect(safeHttpUrl({ href: 'https://example.org/' })).toBeNull();
	});
});
