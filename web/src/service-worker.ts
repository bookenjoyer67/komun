/// <reference types="@sveltejs/kit" />
/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />

import { build, files, version } from '$service-worker';

const sw = self as unknown as ServiceWorkerGlobalScope;
const CACHE_NAME = `komun-${version}`;
const ASSETS = [...build, ...files];

sw.addEventListener('install', (event) => {
	event.waitUntil(
		caches.open(CACHE_NAME)
			.then((cache) => cache.addAll(ASSETS))
			.then(() => sw.skipWaiting())
	);
});

sw.addEventListener('activate', (event) => {
	event.waitUntil(
		caches.keys()
			.then((keys) => Promise.all(
				keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k))
			))
			.then(() => sw.clients.claim())
	);
});

sw.addEventListener('fetch', (event) => {
	const { request } = event;
	const url = new URL(request.url);
	// A worker's fetch is bound by connect-src, so a cross-origin load must stay the page's, under img-src.
	if (url.origin !== sw.location.origin) return;

	if (request.method !== 'GET') return;

	if (ASSETS.includes(url.pathname)) {
		event.respondWith(
			caches.match(request).then((cached) => cached || fetch(request))
		);
		return;
	}

	const CACHEABLE_API_PATHS = ['/api/node', '/api/health', '/api/posts', '/api/directory'];
	const isCacheableApi = CACHEABLE_API_PATHS.some(p => url.pathname.startsWith(p));

	if (isCacheableApi) {
		event.respondWith(
			fetch(request)
				.then((response) => {
					if (response.ok) {
						const clone = response.clone();
						caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
					}
					return response;
				})
				.catch(() =>
					caches.match(request).then((cached) =>
						cached || new Response(JSON.stringify({ error: 'offline' }), {
							status: 503,
							headers: { 'Content-Type': 'application/json' },
						})
					)
				)
		);
		return;
	}

	if (request.mode === 'navigate') {
		event.respondWith(
			fetch(request).catch(() =>
				caches.match('/index.html').then((cached) => cached || caches.match('/'))
			) as Promise<Response>
		);
		return;
	}

	event.respondWith(
		fetch(request).catch(() => caches.match(request)) as Promise<Response>
	);
});
