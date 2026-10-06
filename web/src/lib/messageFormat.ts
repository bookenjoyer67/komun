/**
 * A structured response travels inside the sealed message body, so the server stores it as
 * ciphertext like any other message and needs no schema or API change.
 */
export const RESPONSE_PREFIX = 'komun:response\n';
export const RESPONSE_VERSION = 1;

export const MAX_WHAT_LENGTH = 200;
export const MAX_WHEN_LENGTH = 120;
export const MAX_NOTE_LENGTH = 4000;

export interface ResponseFields {
	what: string;
	when?: string;
	note?: string;
}

export type StructuredResponse = { kind: 'response'; what: string; when?: string; note?: string };
export type DecodedMessage = { kind: 'text'; text: string } | StructuredResponse;

function clip(value: string, max: number): string {
	return value.length > max ? value.slice(0, max) : value;
}

export function encodeResponse(fields: ResponseFields): string {
	const what = fields.what.trim();
	if (!what) throw new Error('A response needs something offered or asked for');
	const envelope: Record<string, unknown> = { v: RESPONSE_VERSION, what: clip(what, MAX_WHAT_LENGTH) };
	const when = fields.when?.trim();
	if (when) envelope.when = clip(when, MAX_WHEN_LENGTH);
	const note = fields.note?.trim();
	if (note) envelope.note = clip(note, MAX_NOTE_LENGTH);
	return RESPONSE_PREFIX + JSON.stringify(envelope);
}

/** `null` means present but not a string, which makes the whole envelope malformed. */
function optionalText(value: unknown, max: number): string | undefined | null {
	if (value === undefined) return undefined;
	if (typeof value !== 'string') return null;
	const trimmed = value.trim();
	return trimmed ? clip(trimmed, max) : undefined;
}

/**
 * Never throws. The body comes from the other party, so anything that is not a well-formed envelope
 * of a known version is returned as the plain text it already was, and every field is capped.
 */
export function decodeMessage(plaintext: string): DecodedMessage {
	const asText: DecodedMessage = { kind: 'text', text: plaintext };
	if (!plaintext.startsWith(RESPONSE_PREFIX)) return asText;

	let parsed: unknown;
	try {
		parsed = JSON.parse(plaintext.slice(RESPONSE_PREFIX.length));
	} catch {
		return asText;
	}
	if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return asText;

	const fields = parsed as Record<string, unknown>;
	if (fields.v !== RESPONSE_VERSION || typeof fields.what !== 'string') return asText;
	const what = fields.what.trim();
	if (!what) return asText;

	const when = optionalText(fields.when, MAX_WHEN_LENGTH);
	const note = optionalText(fields.note, MAX_NOTE_LENGTH);
	if (when === null || note === null) return asText;

	const decoded: StructuredResponse = { kind: 'response', what: clip(what, MAX_WHAT_LENGTH) };
	if (when) decoded.when = when;
	if (note) decoded.note = note;
	return decoded;
}
