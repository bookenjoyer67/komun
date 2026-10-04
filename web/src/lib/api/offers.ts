/**
 * The negotiation trail on a match thread — `match_offers`, append-only. An offer is a numbered
 * step on the encrypted thread, not a message.
 *
 * Unlike the message bodies on the same thread, an offer and its note are server-readable — no
 * plaintext secret, only a price and a sentence — which is what lets the offer list render without
 * a conversation key.
 */
import { getActiveServer } from '$lib/stores/server';
import { getToken } from '$lib/stores/auth';

/** Mirrors `chk_match_offers_kind` (`offer | counter | accept | decline`). */
export type OfferKind = 'offer' | 'counter' | 'accept' | 'decline';

export const OFFER_KINDS = ['offer', 'counter', 'accept', 'decline'] as const satisfies readonly OfferKind[];

export const OFFER_KIND_LABELS: Record<OfferKind, string> = {
	offer: 'Offer',
	counter: 'Counter',
	accept: 'Accepted',
	decline: 'Declined'
};

export interface Offer {
	id: string;
	match_id: string;
	actor_id: string;
	kind: OfferKind;
	amount_cents: number | null;
	currency: string | null;
	note: string | null;
	created_at: string;
}

export interface NewOffer {
	kind: OfferKind;
	amount_cents?: number | null;
	currency?: string | null;
	note?: string | null;
}

/**
 * A server refusal that keeps its status code: a 409 is a legal answer whose sentence the UI must
 * show, while a 400 is a request bug.
 */
export class OfferApiError extends Error {
	readonly status: number;

	constructor(message: string, status: number) {
		super(message);
		this.name = 'OfferApiError';
		this.status = status;
	}
}

async function requestJson<T>(path: string, options: RequestInit = {}): Promise<T> {
	const server = getActiveServer();
	if (!server) throw new OfferApiError('Not connected to a server', 0);

	const headers: Record<string, string> = { 'Content-Type': 'application/json' };
	const token = getToken();
	if (token) headers['Authorization'] = `Bearer ${token}`;

	const res = await fetch(`${server}/api${path}`, {
		...options,
		headers: { ...headers, ...((options.headers as Record<string, string>) || {}) }
	});

	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		throw new OfferApiError(err.error || res.statusText || `Request failed (${res.status})`, res.status);
	}

	if (res.status === 204) return undefined as T;
	return res.json();
}

/** `GET /api/conversations/{id}/offers` — participant-only, oldest first. */
export function listOffers(matchId: string): Promise<Offer[]> {
	return requestJson<Offer[]>(`/conversations/${matchId}/offers`);
}

/**
 * A `decline` carries no amount by server rule; every other kind needs one. Blank notes are
 * omitted so an empty box is "no note", not a note of whitespace.
 */
export function createOffer(matchId: string, offer: NewOffer): Promise<Offer> {
	const body: Record<string, unknown> = { kind: offer.kind };
	if (offer.amount_cents != null) body.amount_cents = offer.amount_cents;
	if (offer.currency) body.currency = offer.currency;
	const note = offer.note?.trim();
	if (note) body.note = note;

	return requestJson<Offer>(`/conversations/${matchId}/offers`, {
		method: 'POST',
		body: JSON.stringify(body)
	});
}

/** The most recent `offer`/`counter` — the number currently on the table, or `null`. */
export function lastNumberedOffer(offers: Offer[]): Offer | null {
	for (let i = offers.length - 1; i >= 0; i -= 1) {
		const kind = offers[i].kind;
		if (kind === 'offer' || kind === 'counter') return offers[i];
	}
	return null;
}

/** The accepted number, read off the last `accept` row, or `null` when nothing was agreed. */
export function agreedOffer(offers: Offer[]): Offer | null {
	for (let i = offers.length - 1; i >= 0; i -= 1) {
		if (offers[i].kind === 'accept') return offers[i];
	}
	return null;
}

/**
 * The counterparty accepts, never the author of the number. The server enforces this too (a
 * self-accept is a 409); the UI does not offer a button whose only outcome is an error.
 */
export function canAccept(offers: Offer[], myUserId: string | null): boolean {
	const last = lastNumberedOffer(offers);
	return last !== null && myUserId !== null && last.actor_id !== myUserId;
}

/** A first number is an `offer`; any number after one is a `counter`. */
export function nextOfferKind(offers: Offer[]): 'offer' | 'counter' {
	return lastNumberedOffer(offers) ? 'counter' : 'offer';
}
