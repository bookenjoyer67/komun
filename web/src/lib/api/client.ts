import { getActiveServer } from '$lib/stores/server';
import { getToken } from '$lib/stores/auth';

function getBase(): string {
	const server = getActiveServer();
	if (!server) throw new Error('Not connected to a server');
	return `${server}/api`;
}

async function request<T>(path: string, options?: RequestInit & { auth?: boolean }): Promise<T> {
	const base = getBase();
	console.log('[api]', options?.method || 'GET', path);
	return requestOn<T>(base, path, options);
}

async function requestOn<T>(base: string, path: string, options?: RequestInit & { auth?: boolean }): Promise<T> {
	const headers: Record<string, string> = {
		'Content-Type': 'application/json',
	};

	if (options?.auth) {
		const token = getToken();
		if (token) {
			headers['Authorization'] = `Bearer ${token}`;
		}
	}

	const res = await fetch(`${base}${path}`, {
		...options,
		headers: { ...headers, ...(options?.headers as Record<string, string> || {}) },
	});

	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		console.error('[api] error:', res.status, path, err.error || res.statusText);
		throw new Error(err.error || 'Request failed');
	}

	return res.json();
}

/** Multipart upload: `fetch` must set `Content-Type` itself so the boundary is correct, so this bypasses `requestOn`. */
async function upload<T>(path: string, form: FormData): Promise<T> {
	const headers: Record<string, string> = {};
	const token = getToken();
	if (token) headers['Authorization'] = `Bearer ${token}`;

	const res = await fetch(`${getBase()}${path}`, { method: 'POST', body: form, headers });
	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		console.error('[api] error:', res.status, path, err.error || res.statusText);
		throw new Error(err.error || 'Upload failed');
	}
	return res.json();
}

/** Every call below corresponds to a route that exists on the flat backend. */
export const api = {
	posts: {
		list: (filters?: Record<string, string>) => {
			const params = new URLSearchParams(
				Object.fromEntries(Object.entries(filters || {}).filter(([, v]) => v))
			).toString();
			const qs = params ? `?${params}` : '';
			return request<any[]>(`/posts${qs}`);
		},
		get: (id: string) => request<any>(`/posts/${id}`),
		create: (data: any) =>
			request<any>('/posts', {
				method: 'POST',
				body: JSON.stringify(data),
				auth: true,
			}),
		update: (id: string, data: { title?: string; body?: string; urgency?: string; status?: string }) =>
			request<any>(`/posts/${id}`, {
				method: 'PATCH',
				body: JSON.stringify(data),
				auth: true,
			}),
		delete: (id: string) =>
			request<any>(`/posts/${id}`, {
				method: 'DELETE',
				auth: true,
			}).catch((e: Error) => { console.error('[delete] withdraw failed:', e.message); throw e; }),
		addImages: (id: string, files: File[]) => {
			const form = new FormData();
			for (const file of files) form.append('images', file);
			return upload<{ images: string[] }>(`/posts/${id}/images`, form);
		},
	},

	conversations: {
		list: () => request<any[]>('/me/conversations', { auth: true }),
		get: (matchId: string) => request<any>(`/conversations/${matchId}`, { auth: true }),
		/**
		 * The server stores an opaque blob and never sees plaintext, so this takes a ciphertext.
		 * `nonce` is optional because `encryptMessage` prepends the 24-byte XChaCha20 nonce.
		 */
		respond: (postId: string, ciphertext: string, serverUrl?: string) => {
			const base = serverUrl ? `${serverUrl}/api` : getBase();
			return requestOn<{ match_id: string }>(base, `/posts/${postId}/respond`, {
				method: 'POST',
				body: JSON.stringify({ ciphertext }),
				auth: true,
			});
		},
		sendMessage: (matchId: string, ciphertext: string) =>
			request<any>(`/conversations/${matchId}/messages`, {
				method: 'POST',
				body: JSON.stringify({ ciphertext }),
				auth: true,
			}),
		updateStatus: (matchId: string, status: string) =>
			request<any>(`/conversations/${matchId}/status`, {
				method: 'PATCH',
				body: JSON.stringify({ status }),
				auth: true,
			}),
	},

	endorsements: {
		list: (userId: string) =>
			request<{ count: number; endorsements: any[] }>(`/users/${userId}/endorsements`),
		endorse: (userId: string, note?: string) =>
			request<any>(`/users/${userId}/endorse`, {
				method: 'POST',
				body: JSON.stringify({ note }),
				auth: true,
			}),
		unendorse: (userId: string) =>
			request<any>(`/users/${userId}/endorse`, {
				method: 'DELETE',
				auth: true,
			}),
	},

	admin: {
		stats: () => request<any>('/admin/stats', { auth: true }),
		listUsers: () => request<any[]>('/admin/users', { auth: true }),
		deleteUser: (id: string) => request<any>(`/admin/users/${id}`, { method: 'DELETE', auth: true }),
		changeRole: (id: string, role: string) => request<any>(`/admin/users/${id}/role`, {
			method: 'PATCH',
			body: JSON.stringify({ role }),
			auth: true,
		}),
		listDirectory: () => request<any[]>('/admin/directory', { auth: true }),
		removeDirectoryEntry: (url: string) => request<any>(`/admin/directory/${encodeURIComponent(url)}`, { method: 'DELETE', auth: true }),
	},
};
