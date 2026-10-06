import { describe, it, expect } from 'vitest';
import {
	RESPONSE_PREFIX,
	MAX_WHAT_LENGTH,
	MAX_WHEN_LENGTH,
	encodeResponse,
	decodeMessage
} from '$lib/messageFormat';

/** Every value here is an invented test sentence. */

describe('structured response envelope', () => {
	it('round-trips what, when and a note', () => {
		const encoded = encodeResponse({
			what: 'A ladder and two hours of help',
			when: 'Saturday morning',
			note: 'I live two streets away.'
		});
		expect(encoded.startsWith(RESPONSE_PREFIX)).toBe(true);
		expect(decodeMessage(encoded)).toEqual({
			kind: 'response',
			what: 'A ladder and two hours of help',
			when: 'Saturday morning',
			note: 'I live two streets away.'
		});
	});

	it('round-trips without the optional when and note', () => {
		const decoded = decodeMessage(encodeResponse({ what: 'Spare seedlings', when: '  ', note: '' }));
		expect(decoded).toEqual({ kind: 'response', what: 'Spare seedlings' });
	});

	it('refuses to encode an empty what', () => {
		expect(() => encodeResponse({ what: '   ' })).toThrow();
	});

	it('a plain message is not mistaken for a structured response', () => {
		for (const plain of [
			'Hi, I can help on Saturday.',
			'{"v":1,"what":"looks like JSON but has no prefix"}',
			'',
			RESPONSE_PREFIX.trim()
		]) {
			expect(decodeMessage(plain)).toEqual({ kind: 'text', text: plain });
		}
	});

	it('a malformed envelope degrades to plain text and never throws', () => {
		const malformed = [
			`${RESPONSE_PREFIX}not json`,
			`${RESPONSE_PREFIX}null`,
			`${RESPONSE_PREFIX}[1,2]`,
			`${RESPONSE_PREFIX}"a string"`,
			`${RESPONSE_PREFIX}{"v":1}`,
			`${RESPONSE_PREFIX}{"v":1,"what":"   "}`,
			`${RESPONSE_PREFIX}{"v":1,"what":42}`,
			`${RESPONSE_PREFIX}{"v":1,"what":"ok","when":7}`,
			`${RESPONSE_PREFIX}{"v":1,"what":"ok","note":{}}`,
			`${RESPONSE_PREFIX}{"v":99,"what":"from a future version"}`,
			`${RESPONSE_PREFIX}{"what":"no version"}`
		];
		for (const body of malformed) {
			expect(() => decodeMessage(body)).not.toThrow();
			expect(decodeMessage(body)).toEqual({ kind: 'text', text: body });
		}
	});

	it('caps decoded field lengths, because the envelope comes from the other party', () => {
		const body = `${RESPONSE_PREFIX}${JSON.stringify({
			v: 1,
			what: 'w'.repeat(MAX_WHAT_LENGTH * 3),
			when: 'n'.repeat(MAX_WHEN_LENGTH * 3)
		})}`;
		const decoded = decodeMessage(body);
		expect(decoded.kind).toBe('response');
		if (decoded.kind !== 'response') return;
		expect(decoded.what).toHaveLength(MAX_WHAT_LENGTH);
		expect(decoded.when).toHaveLength(MAX_WHEN_LENGTH);
	});
});
