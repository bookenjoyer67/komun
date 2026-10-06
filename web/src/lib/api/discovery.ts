import { get } from 'svelte/store';
import { getDirectories } from '$lib/stores/directories';
import { getLocation } from '$lib/stores/location';
import { getActiveServer, isServingOrigin, serverState } from '$lib/stores/server';
import { coarsenCoordinate } from '$lib/geo';
import type { PostLike } from '$lib/api/types';

function haversineKm(lat1: number, lon1: number, lat2: number, lon2: number): number {
	const R = 6371;
	const dLat = (lat2 - lat1) * Math.PI / 180;
	const dLon = (lon2 - lon1) * Math.PI / 180;
	const a = Math.sin(dLat / 2) ** 2 +
		Math.cos(lat1 * Math.PI / 180) * Math.cos(lat2 * Math.PI / 180) *
		Math.sin(dLon / 2) ** 2;
	return R * 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
}

/** A server advertising itself lists one location, its own. */
export interface NearbyServer {
	url: string;
	name: string;
	description?: string;
	location_name?: string;
	location_lat?: number;
	location_lon?: number;
	distance_km?: number;
}

export interface AggregatedPost extends PostLike {
	server_url: string;
	server_name: string;
	server_location?: string;
}

export interface DiscoveryResult {
	servers: NearbyServer[];
	posts: AggregatedPost[];
}

function toServer(entry: any): NearbyServer {
	return {
		url: entry.url,
		name: entry.name,
		description: entry.description,
		location_name: entry.location_name,
		location_lat: entry.location_lat ?? undefined,
		location_lon: entry.location_lon ?? undefined,
		distance_km: entry.distance_km,
	};
}

/**
 * A directory lists only servers that registered with it, under their public URL, so the server
 * this app talks to can be missing from its own list. Naming it costs no request.
 */
function activeSource(): NearbyServer | null {
	const url = getActiveServer();
	if (!url || !isServingOrigin(url)) return null;
	const known = get(serverState).known.find((s) => s.url === url);
	return { url, name: known?.name || new URL(url).host };
}

export async function discoverNearbyServers(): Promise<NearbyServer[]> {
	const dirs = getDirectories();
	const loc = getLocation();

	if (!loc.lat || !loc.lon) return [];

	// A directory is someone else's server: it gets the grid cell, never the exact point.
	const lat = coarsenCoordinate(loc.lat);
	const lon = coarsenCoordinate(loc.lon);

	const results = await Promise.allSettled(
		dirs.map(async (dirUrl) => {
			const url = `${dirUrl}/api/directory?lat=${lat}&lon=${lon}&radius=50`;
			const res = await fetch(url);
			if (!res.ok) return [];
			const data = await res.json();
			return data.map(toServer);
		})
	);

	const allServers: NearbyServer[] = [];
	const seen = new Set<string>();

	for (const result of results) {
		if (result.status === 'fulfilled') {
			for (const server of result.value) {
				if (!seen.has(server.url)) {
					seen.add(server.url);
					allServers.push(server);
				}
			}
		}
	}

	allServers.sort((a, b) => (a.distance_km ?? 999) - (b.distance_km ?? 999));

	const active = activeSource();
	if (!active) return allServers.slice(0, 5);
	const listed = allServers.find((s) => s.url === active.url) ?? active;
	return [listed, ...allServers.filter((s) => s.url !== active.url)].slice(0, 5);
}

export async function discoverAllServers(): Promise<NearbyServer[]> {
	const dirs = getDirectories();

	const results = await Promise.allSettled(
		dirs.map(async (dirUrl) => {
			const res = await fetch(`${dirUrl}/api/directory`);
			if (!res.ok) return [];
			return (await res.json()).map(toServer);
		})
	);

	const allServers: NearbyServer[] = [];
	const seen = new Set<string>();

	for (const result of results) {
		if (result.status === 'fulfilled') {
			for (const server of result.value) {
				if (!seen.has(server.url)) {
					seen.add(server.url);
					allServers.push(server);
				}
			}
		}
	}

	return allServers;
}

/**
 * One request per server. A post without coordinates falls back to the server's advertised
 * location, and is kept if neither is known rather than silently dropped.
 */
export async function fetchFromServers(
	servers: NearbyServer[],
	searchCenter?: { lat: number; lon: number; radiusKm: number }
): Promise<DiscoveryResult> {
	const allPosts: AggregatedPost[] = [];

	const results = await Promise.allSettled(
		servers.map(async (server) => {
			const res = await fetch(`${server.url}/api/posts`);
			if (!res.ok) return [];

			const posts: any[] = await res.json();
			const kept: AggregatedPost[] = [];

			for (const p of posts) {
				if (searchCenter) {
					const lat = p.location_lat ?? server.location_lat;
					const lon = p.location_lon ?? server.location_lon;
					if (lat != null && lon != null) {
						if (haversineKm(searchCenter.lat, searchCenter.lon, lat, lon) > searchCenter.radiusKm) continue;
					}
				}
				kept.push({
					...p,
					server_url: server.url,
					server_name: server.name,
					server_location: server.location_name,
				});
				if (kept.length >= 50) break;
			}

			return kept;
		})
	);

	for (const result of results) {
		if (result.status === 'fulfilled') allPosts.push(...result.value);
	}

	allPosts.sort((a, b) => {
		const urgencyOrder: Record<string, number> = { critical: 0, high: 1, medium: 2, low: 3 };
		const ua = urgencyOrder[a.urgency || 'low'] ?? 3;
		const ub = urgencyOrder[b.urgency || 'low'] ?? 3;
		if (ua !== ub) return ua - ub;
		return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
	});

	return { servers, posts: allPosts };
}
