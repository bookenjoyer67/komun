import { describe, it, expect } from 'vitest';
import { LOCATION_PRECISION_DEGREES, coarsenCoordinate } from '$lib/geo';

/** The browser's copy of the server's grid: both are pinned to 0.1, each by its own test. */
describe('location grid', () => {
	it('is a tenth of a degree', () => {
		expect(LOCATION_PRECISION_DEGREES).toBe(0.1);
	});

	// `37.8`, not `37.800000000000004`: multiplying a step count by 0.1 lands off the grid.
	it('snaps a coordinate to the grid and prints it short', () => {
		expect(coarsenCoordinate(37.80443)).toBe(37.8);
		expect(String(coarsenCoordinate(37.80443))).toBe('37.8');
		expect(coarsenCoordinate(-122.27121)).toBe(-122.3);
		expect(String(coarsenCoordinate(-122.27121))).toBe('-122.3');
	});

	it('leaves the boundaries where they are', () => {
		for (const edge of [90, -90, 180, -180]) {
			expect(coarsenCoordinate(edge)).toBe(edge);
		}
	});

	it('is idempotent', () => {
		for (const raw of [37.80443, -122.27121, 0.05, -0.04, 89.96, -179.95, 12.345678]) {
			const once = coarsenCoordinate(raw);
			expect(coarsenCoordinate(once)).toBe(once);
		}
	});

	// `toBe` compares with Object.is, so -0 fails it; a served -0 would say which side of the line.
	it('rounds a value near zero to positive zero', () => {
		expect(coarsenCoordinate(-0.04)).toBe(0);
	});

	// `122.25` is exact in binary, so this is a true tie. `crates/core/src/tests.rs` pins the same
	// two values against the server's `f64::round`, so the runtimes cannot drift apart unnoticed.
	it('rounds a tie away from zero, as the server does', () => {
		expect(coarsenCoordinate(122.25)).toBe(122.3);
		expect(coarsenCoordinate(-122.25)).toBe(-122.3);
	});
});
