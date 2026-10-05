<script module lang="ts">
	export interface MapMarker {
		lat: number;
		lon: number;
		label?: string;
	}

	export interface PickedCoords {
		lat: number;
		lon: number;
	}

	/** Clamp a pair to a valid latitude/longitude, or `null` for anything non-finite. */
	export function clampLatLon(lat: unknown, lon: unknown): PickedCoords | null {
		const la = typeof lat === 'number' ? lat : Number(lat);
		const lo = typeof lon === 'number' ? lon : Number(lon);
		if (!Number.isFinite(la) || !Number.isFinite(lo)) return null;
		return {
			lat: Math.min(90, Math.max(-90, la)),
			lon: Math.min(180, Math.max(-180, lo))
		};
	}

	/**
	 * Leaflet assigns string popup content with innerHTML and appends an element as-is, so an
	 * untrusted label must reach `bindPopup` as an element holding text, never as a string.
	 */
	export function popupContent(label: string): HTMLElement {
		const node = document.createElement('span');
		node.textContent = label;
		return node;
	}
</script>

<script lang="ts">
	// Leaflet has no TypeScript declarations here and none can be installed (no egress); keep the
	// import untyped rather than add a diagnostic to the frozen baseline.
	// @ts-ignore
	import * as L from 'leaflet';
	import 'leaflet/dist/leaflet.css';

	interface Props {
		lat: number;
		lon: number;
		zoom?: number;
		tileUrl?: string;
		attribution?: string;
		markers?: MapMarker[];
		/** Opt-in click-to-place mode. Off by default, so `/map` stays read-only. */
		pickable?: boolean;
		/** The current picked coordinates, drawn as an extra marker when `pickable`. */
		pickedLat?: number | null;
		pickedLon?: number | null;
		/** Called with clamped coordinates when the user clicks the map in pick mode. */
		onpick?: (coords: PickedCoords) => void;
	}

	let {
		lat,
		lon,
		zoom = 13,
		tileUrl = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
		attribution = '&copy; OpenStreetMap contributors',
		markers = [],
		pickable = false,
		pickedLat = null,
		pickedLon = null,
		onpick
	}: Props = $props();

	let container = $state<HTMLDivElement | null>(null);
	// Raw state: a Leaflet object must not be wrapped in a deep reactive proxy.
	let markerLayer = $state.raw<L.LayerGroup | null>(null);

	const markerIcon = L.divIcon({
		className: 'komun-map-marker',
		iconSize: [14, 14],
		iconAnchor: [7, 7]
	});

	$effect(() => {
		if (!container) return;

		const instance = L.map(container, { attributionControl: false });
		instance.setView([lat, lon], zoom);

		// Attribution is rendered unconditionally, so it is present even when
		// no tile ever loads (bogus URL, offline, blocked host).
		L.control
			.attribution({ prefix: false })
			.addAttribution(attribution)
			.addTo(instance);

		try {
			L.tileLayer(tileUrl, { attribution: '' }).addTo(instance);
		} catch (err) {
			// A malformed tile URL must degrade to an empty map, never a broken page.
			console.warn('[LocationMap] invalid tile URL, showing an empty map', err);
		}

		if (pickable) {
			instance.on('click', (event: unknown) => {
				const latlng = (event as { latlng?: { lat: unknown; lng: unknown } })?.latlng;
				const coords = clampLatLon(latlng?.lat, latlng?.lng);
				if (coords) onpick?.(coords);
			});
		}

		markerLayer = L.layerGroup().addTo(instance);

		return () => {
			markerLayer = null;
			instance.remove();
		};
	});

	// Markers live in their own effect so updating a pin does not tear down and rebuild the map.
	$effect(() => {
		const layer = markerLayer;
		if (!layer) return;

		layer.clearLayers();
		for (const marker of markers) {
			L.marker([marker.lat, marker.lon], { icon: markerIcon })
				.bindPopup(popupContent(marker.label ?? ''))
				.addTo(layer);
		}
		if (pickable && pickedLat != null && pickedLon != null) {
			L.marker([pickedLat, pickedLon], { icon: markerIcon }).addTo(layer);
		}
	});
</script>

<div class="location-map" bind:this={container} data-attribution={attribution}></div>

<style>
	.location-map {
		width: 100%;
		height: 100%;
		min-height: 320px;
		border-radius: var(--radius, 8px);
		overflow: hidden;
		background: var(--bg-surface, #eee);
	}

	/* Leaflet builds these nodes itself, so the selector must be global. */
	:global(.komun-map-marker) {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		background: var(--accent, #d95d39);
		border: 2px solid #fff;
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
	}
</style>
