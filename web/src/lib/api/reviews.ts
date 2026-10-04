/**
 * Deal reviews — `deal_reviews`, writable only against a completed deal.
 *
 * Participant-only, one review per (match, reviewer), only when the match is `completed`; the
 * rating is a 1–5 integer, validated here before sending.
 */
import { getActiveServer } from '$lib/stores/server';
import { getToken } from '$lib/stores/auth';

export const MIN_RATING = 1;
export const MAX_RATING = 5;
export const MAX_REVIEW_BODY = 2000;

export const ALREADY_REVIEWED_MESSAGE = 'you have already reviewed this deal';

/** A review as returned to its author (`POST /api/matches/{id}/reviews`). */
export interface Review {
	id: string;
	match_id: string;
	reviewer_id: string;
	reviewee_id: string;
	rating: number;
	body: string | null;
	created_at: string;
}

/** A review as it appears on a profile (`GET /api/users/{id}/reviews`) — attributed. */
export interface ReviewView {
	id: string;
	match_id: string;
	reviewer_id: string;
	reviewer_display_name: string;
	rating: number;
	body: string | null;
	created_at: string;
}

export interface NewReview {
	rating: number;
	body?: string | null;
}

export class ReviewApiError extends Error {
	readonly status: number;

	constructor(message: string, status: number) {
		super(message);
		this.name = 'ReviewApiError';
		this.status = status;
	}
}

async function requestJson<T>(path: string, options: RequestInit = {}): Promise<T> {
	const server = getActiveServer();
	if (!server) throw new ReviewApiError('Not connected to a server', 0);

	const headers: Record<string, string> = { 'Content-Type': 'application/json' };
	const token = getToken();
	if (token) headers['Authorization'] = `Bearer ${token}`;

	const res = await fetch(`${server}/api${path}`, {
		...options,
		headers: { ...headers, ...((options.headers as Record<string, string>) || {}) }
	});

	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		throw new ReviewApiError(
			err.error || res.statusText || `Request failed (${res.status})`,
			res.status
		);
	}

	if (res.status === 204) return undefined as T;
	return res.json();
}

/** Count by Unicode code point, the way the server counts with `chars().count()`. */
export function countChars(value: string): number {
	return [...value].length;
}

/** `null` when the rating is a whole number in range, otherwise the sentence to show. */
export function validateRating(rating: number): string | null {
	if (!Number.isInteger(rating) || rating < MIN_RATING || rating > MAX_RATING) {
		return `Choose a rating from ${MIN_RATING} to ${MAX_RATING}.`;
	}
	return null;
}

/** `null` when the body fits, otherwise the sentence to show. */
export function validateBody(body: string): string | null {
	if (countChars(body) > MAX_REVIEW_BODY) {
		return `Your review must be ${MAX_REVIEW_BODY} characters or fewer.`;
	}
	return null;
}

/** `POST /api/matches/{id}/reviews` — 409 when the deal is not completed or already reviewed. */
export function createReview(matchId: string, review: NewReview): Promise<Review> {
	const body: Record<string, unknown> = { rating: review.rating };
	const text = review.body?.trim();
	if (text) body.body = text;

	return requestJson<Review>(`/matches/${matchId}/reviews`, {
		method: 'POST',
		body: JSON.stringify(body)
	});
}

/** `GET /api/users/{id}/reviews` — public, newest first. */
export function listUserReviews(
	userId: string,
	page: { limit?: number; offset?: number } = {}
): Promise<ReviewView[]> {
	const params = new URLSearchParams();
	if (page.limit != null) params.set('limit', String(page.limit));
	if (page.offset != null) params.set('offset', String(page.offset));
	const qs = params.toString();
	return requestJson<ReviewView[]>(`/users/${userId}/reviews${qs ? `?${qs}` : ''}`);
}
