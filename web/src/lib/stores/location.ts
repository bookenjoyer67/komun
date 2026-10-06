import { writable, get } from 'svelte/store';
import { getActiveServer } from '$lib/stores/server';

interface LocationState {
	name: string;
	lat: number | null;
	lon: number | null;
}

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
export async function geocode(query: string): Promise<boolean> {
	const serverUrl = getActiveServer();
	if (!serverUrl) return false;

	try {
		const encoded = encodeURIComponent(query);
		const res = await fetch(`${serverUrl}/api/geocode?q=${encoded}`);
		if (!res.ok) return false;

		const result = await res.json();
		if (!result.lat || !result.lon) return false;

		location.set({
			name: result.display_name.split(',').slice(0, 2).join(',').trim(),
			lat: parseFloat(result.lat),
			lon: parseFloat(result.lon),
		});
		return true;
	} catch {
		return false;
	}
}

export function clearLocation() {
	location.set({ name: '', lat: null, lon: null });
}
