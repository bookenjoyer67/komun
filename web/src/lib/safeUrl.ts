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

/**
 * A same-origin absolute path fit for the SPA router, or `null`. `safeHttpUrl` is the wrong rule
 * for an in-app link: it admits any host and refuses the bare path every legitimate link is. A
 * second character of `/` or `\` is refused before parsing, because the URL parser reads both
 * `//host` and `/\host` as a host rather than a path. The parsed path is returned rather than the
 * input, so the path checked is the path the router receives.
 */
export function safeAppPath(raw: unknown, origin: string): string | null {
	if (typeof raw !== 'string') return null;
	const trimmed = raw.trim();
	if (!trimmed.startsWith('/') || trimmed[1] === '/' || trimmed[1] === '\\') return null;
	let url: URL;
	try {
		url = new URL(trimmed, origin);
		if (url.origin !== new URL(origin).origin) return null;
	} catch {
		return null;
	}
	return url.pathname + url.search + url.hash;
}
