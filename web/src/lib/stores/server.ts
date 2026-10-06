import { writable, get } from 'svelte/store';

export interface NodeInfo {
	name: string;
	description: string;
	version: string;
	domain?: string;
	location?: { name?: string; lat?: number; lon?: number };
	listed: boolean;
}

export interface KnownServer {
	url: string;
	name: string;
	description: string;
	domain?: string;
	lastSeen: number;
}

interface ServerState {
	active: string | null;
	known: KnownServer[];
}

const STORAGE_KEY = 'komun_servers';

function servingOrigin(): string | null {
	return typeof window === 'undefined' ? null : window.location.origin;
}

/**
 * Unless the build names another server, or opts out with 'none', an empty choice falls back to
 * the origin that served the app: the only one connect-src allows.
 */
function defaultServer(): string | null {
	const configured = String(import.meta.env.VITE_DEFAULT_SERVER ?? '').trim();
	if (configured === 'none') return null;
	if (configured) return configured.replace(/\/+$/, '');
	return servingOrigin();
}

/** True when url is on the origin that served this page, the only one connect-src allows (D13); a caller refuses any other before sending. */
export function isServingOrigin(url: string): boolean {
	const own = servingOrigin();
	if (!own) return false;
	try {
		return new URL(url).origin === own;
	} catch {
		return false;
	}
}

function readStoredState(): ServerState {
	if (typeof localStorage === 'undefined') return { active: null, known: [] };
	const raw = localStorage.getItem(STORAGE_KEY);
	if (!raw) return { active: null, known: [] };
	try {
		const parsed = JSON.parse(raw) as ServerState;
		if (parsed.known) {
			for (const s of parsed.known) {
				if (!s.domain) s.domain = extractDomain(s.url);
			}
		}
		return parsed;
	} catch {
		return { active: null, known: [] };
	}
}

/** A visitor with known servers and none active chose that state, so only a blank one is seeded. */
function loadFromStorage(): ServerState {
	const state = readStoredState();
	if (!state.active && !state.known?.length) {
		return { active: defaultServer(), known: state.known ?? [] };
	}
	return state;
}

function saveToStorage(state: ServerState) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

function extractDomain(url: string): string {
	const host = url.replace(/^https?:\/\//, '').split('/')[0];
	return host.split(':')[0];
}

export const serverState = writable<ServerState>(loadFromStorage());

serverState.subscribe(saveToStorage);

export function getActiveServer(): string | null {
	return get(serverState).active;
}

export function getServerByDomain(domain: string): KnownServer | null {
	const state = get(serverState);
	return state.known.find(s => s.domain === domain) || null;
}

export function isConnected(): boolean {
	return get(serverState).active !== null;
}

function notFullAddress(input: string): Error {
	const example = input.includes('://') ? '' : `, for example https://${input}`;
	return new Error(`Not connected: “${input}” is not a full server address. Include https://${example}.`);
}

function notKomun(origin: string): Error {
	return new Error(`Not connected: ${origin} did not answer as a Komun server.`);
}

export async function connectToServer(url: string): Promise<NodeInfo> {
	const input = url.trim();
	let target: URL;
	try {
		target = new URL(input);
	} catch {
		throw notFullAddress(input);
	}
	if (target.protocol !== 'https:' && target.protocol !== 'http:') throw notFullAddress(input);

	if (!isServingOrigin(target.href)) {
		throw new Error(
			`Not connected: this app only talks to the server it was loaded from (${servingOrigin()}). ` +
				`To use ${target.origin}, open that address in your browser.`
		);
	}

	const normalized = input.replace(/\/+$/, '');

	let res: Response;
	try {
		res = await fetch(`${normalized}/api/node`);
	} catch {
		throw new Error(`Not connected: could not reach ${target.origin}.`);
	}
	if (!res.ok) throw notKomun(target.origin);

	let info: NodeInfo;
	try {
		info = await res.json();
	} catch {
		throw notKomun(target.origin);
	}

	serverState.update((state) => {
		const existing = state.known.findIndex((s) => s.url === normalized);
		const entry: KnownServer = {
			url: normalized,
			name: info.name,
			description: info.description,
			domain: info.domain,
			lastSeen: Date.now(),
		};

		if (existing >= 0) {
			state.known[existing] = entry;
		} else {
			state.known.push(entry);
		}

		return { active: normalized, known: state.known };
	});

	return info;
}

export function disconnectServer() {
	serverState.update((state) => ({ ...state, active: null }));
}

export function removeServer(url: string) {
	serverState.update((state) => ({
		active: state.active === url ? null : state.active,
		known: state.known.filter((s) => s.url !== url),
	}));
}
