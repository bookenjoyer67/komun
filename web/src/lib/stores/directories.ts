import { writable, get } from 'svelte/store';

/** The legacy key holds only auto-saved build defaults, never a user choice, so it is dropped unread. */
const LEGACY_KEY = 'komun_directories';
const STORAGE_KEY = 'komun_chosen_directories';

function defaultDirectories(): string[] {
	const configured = String(import.meta.env.VITE_DEFAULT_DIRECTORY ?? '').trim();
	if (configured) return [configured.replace(/\/+$/, '')];
	return typeof window === 'undefined' ? [] : [window.location.origin];
}

function loadFromStorage(): string[] {
	if (typeof localStorage === 'undefined') return defaultDirectories();
	localStorage.removeItem(LEGACY_KEY);
	const raw = localStorage.getItem(STORAGE_KEY);
	if (!raw) return defaultDirectories();
	try {
		const parsed = JSON.parse(raw);
		if (!Array.isArray(parsed) || parsed.length === 0) return defaultDirectories();
		if (!parsed.every((d) => typeof d === 'string')) return defaultDirectories();
		return parsed;
	} catch {
		return defaultDirectories();
	}
}

function saveToStorage(dirs: string[]) {
	if (typeof localStorage === 'undefined') return;
	localStorage.setItem(STORAGE_KEY, JSON.stringify(dirs));
}

/** Only an explicit add or remove is persisted, so a later build default still reaches this visitor. */
export const directories = writable<string[]>(loadFromStorage());

export function addDirectory(url: string) {
	const normalized = url.replace(/\/+$/, '');
	directories.update((dirs) => (dirs.includes(normalized) ? dirs : [...dirs, normalized]));
	saveToStorage(get(directories));
}

export function removeDirectory(url: string) {
	directories.update((dirs) => dirs.filter((d) => d !== url));
	saveToStorage(get(directories));
}

export function getDirectories(): string[] {
	return get(directories);
}
