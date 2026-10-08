import { describe, it, expect } from 'vitest';
import { safeHttpUrl, safeAppPath } from '$lib/safeUrl';

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

describe('safeAppPath (R8)', () => {
	const ORIGIN = 'https://app.test';

	it('accepts a same-origin absolute path and returns the parsed path', () => {
		const link = '/messages/0190a0b0-0000-7000-8000-000000000001';
		expect(safeAppPath(link, ORIGIN)).toBe(link);
		expect(safeAppPath('  /messages/1  ', ORIGIN)).toBe('/messages/1');
	});

	it('keeps the query and fragment', () => {
		expect(safeAppPath('/search?q=a#h', ORIGIN)).toBe('/search?q=a#h');
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
	])('rejects %s', (_name, input) => {
		expect(safeAppPath(input, ORIGIN)).toBeNull();
	});

	it('rejects non-string input', () => {
		expect(safeAppPath(undefined, ORIGIN)).toBeNull();
		expect(safeAppPath(null, ORIGIN)).toBeNull();
		expect(safeAppPath(42, ORIGIN)).toBeNull();
		expect(safeAppPath({ pathname: '/messages/1' }, ORIGIN)).toBeNull();
	});
});
