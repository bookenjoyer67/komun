import { writable, get } from 'svelte/store';
import { getActiveServer, isServingOrigin } from '$lib/stores/server';

interface LocationState {
	name: string;
	lat: number | null;
	lon: number | null;
}

export type LocationOutcome =
	| { kind: 'found' }
	| { kind: 'no-server' }
	| { kind: 'refused'; server: string }
	| { kind: 'unreachable'; server: string }
	| { kind: 'unavailable'; server: string }
	| { kind: 'busy' }
	| { kind: 'not-found' };

const STORAGE_KEY = 'komun_location';

function loadFromStorage(): LocationState {
	if (typeof localStorage === 'undefined') return { name: '', lat: null, lon: null };
	const raw = localStorage.getItem(STORAGE_KEY);
	if (!raw) return { name: '', lat: null, lon: null };
	try {
		return JSON.parse(raw);
	} catch {
		return { name: '', lat: null, lon: null };
	}
}

function saveToStorage(state: LocationState) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export const location = writable<LocationState>(loadFromStorage());

location.subscribe(saveToStorage);

export function hasLocation(): boolean {
	const loc = get(location);
	return loc.lat !== null && loc.lon !== null;
}

export function getLocation(): LocationState {
	return get(location);
}

/**
 * The query is what the user typed, so it goes to the active server or nowhere. A directory is
 * someone else's server and is never a fallback.
 */
export async function searchLocation(query: string): Promise<LocationOutcome> {
	const server = getActiveServer();
	if (!server) return { kind: 'no-server' };
	if (!isServingOrigin(server)) return { kind: 'refused', server };

	let res: Response;
	try {
		res = await fetch(`${server}/api/geocode?q=${encodeURIComponent(query)}`);
	} catch {
		return { kind: 'unreachable', server };
	}
	if (res.status === 429 || res.status === 503) return { kind: 'busy' };
	if (res.status === 400 || res.status === 404) return { kind: 'not-found' };
	if (!res.ok) return { kind: 'unavailable', server };

	let result: any;
	try {
		result = await res.json();
	} catch {
		return { kind: 'unavailable', server };
	}
	if (!result?.lat || !result?.lon) return { kind: 'not-found' };

	location.set({
		name: String(result.display_name ?? query).split(',').slice(0, 2).join(',').trim(),
		lat: parseFloat(result.lat),
		lon: parseFloat(result.lon),
	});
	return { kind: 'found' };
}

export async function geocode(query: string): Promise<boolean> {
	return (await searchLocation(query)).kind === 'found';
}

export function clearLocation() {
	location.set({ name: '', lat: null, lon: null });
}
