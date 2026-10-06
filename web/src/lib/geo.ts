/**
 * Every coordinate this client sends to another server or puts on a map sits on this grid;
 * changing the precision is this line. It must divide one degree into a whole number of steps.
 * The server holds its own copy in `crates/core/src/models/post.rs`.
 */
export const LOCATION_PRECISION_DEGREES = 0.1;

const STEPS_PER_DEGREE = Math.round(1 / LOCATION_PRECISION_DEGREES);

/**
 * Divides a whole step count rather than multiplying by the step, because `378 * 0.1` is
 * `37.800000000000004`. A tie rounds away from zero to match the server's `f64::round`; bare
 * `Math.round` would send `-122.25` to `-122.2` where the server gives `-122.3`.
 */
export function coarsenCoordinate(value: number): number {
	const coarse =
		(Math.sign(value) * Math.round(Math.abs(value) * STEPS_PER_DEGREE)) / STEPS_PER_DEGREE;
	// A -0 would tell a reader which side of the line the exact value sat on.
	return coarse === 0 ? 0 : coarse;
}
