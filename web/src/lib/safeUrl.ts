/**
 * An absolute http(s) URL fit for an `href`, or `null`. Any other scheme can execute or render
 * content when followed, so it never reaches an `href`; with no base URL, a relative input is
 * rejected too. The parsed `href` is returned rather than the input, because the scheme checked
 * must be the one the browser resolves (the URL parser drops whitespace a raw prefix test would
 * miss).
 */
export function safeHttpUrl(raw: unknown): string | null {
	if (typeof raw !== 'string') return null;
	let url: URL;
	try {
		url = new URL(raw.trim());
	} catch {
		return null;
	}
	return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
}
