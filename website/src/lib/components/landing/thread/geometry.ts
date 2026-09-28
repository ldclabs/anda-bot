// Pure geometry and scheduling for the landing-page journey: Anda travels a
// thread through the page. The DOM engine in `engine.ts` measures anchors and
// renders SVG; everything here is plain math so the choreography stays
// deterministic and testable.

export type ThreadAnchor = {
	x: number;
	y: number;
	/** Unit tangent the thread follows while passing the anchor. */
	tx: number;
	ty: number;
	/** Radius of a knot tied at the anchor; the sign picks the side. */
	loop: number;
};

const KAPPA = 0.5522847498;

function n(value: number) {
	return Math.round(value * 10) / 10;
}

export function clamp(value: number, min = 0, max = 1) {
	return value < min ? min : value > max ? max : value;
}

export function smoothstep(edge0: number, edge1: number, value: number) {
	const t = clamp((value - edge0) / (edge1 - edge0));
	return t * t * (3 - 2 * t);
}

/** Cubic arcs of a full knot that leaves and re-enters `a` along its tangent. */
export function knotPath(a: ThreadAnchor): string {
	const r = Math.abs(a.loop);
	if (r < 0.5) return '';
	const side = a.loop < 0 ? -1 : 1;
	// `u` points from the anchor to the knot centre, perpendicular to travel.
	const ux = -a.ty * side;
	const uy = a.tx * side;
	const cx = a.x + ux * r;
	const cy = a.y + uy * r;
	const point = (theta: number) => [
		cx + r * (-Math.cos(theta) * ux + Math.sin(theta) * a.tx),
		cy + r * (-Math.cos(theta) * uy + Math.sin(theta) * a.ty)
	];
	const direction = (theta: number) => [
		Math.sin(theta) * ux + Math.cos(theta) * a.tx,
		Math.sin(theta) * uy + Math.cos(theta) * a.ty
	];
	let d = '';
	for (let quarter = 0; quarter < 4; quarter += 1) {
		const from = (quarter * Math.PI) / 2;
		const to = from + Math.PI / 2;
		const [x0, y0] = point(from);
		const [x1, y1] = point(to);
		const [dx0, dy0] = direction(from);
		const [dx1, dy1] = direction(to);
		d += ` C${n(x0 + dx0 * KAPPA * r)} ${n(y0 + dy0 * KAPPA * r)} ${n(x1 - dx1 * KAPPA * r)} ${n(
			y1 - dy1 * KAPPA * r
		)} ${n(x1)} ${n(y1)}`;
	}
	return d;
}

/** One stretch of thread: an optional knot at `a`, then a smooth run to `b`. */
export function segmentPath(a: ThreadAnchor, b: ThreadAnchor): string {
	const distance = Math.hypot(b.x - a.x, b.y - a.y);
	const handle = clamp(distance * 0.5, 24, 520);
	return (
		`M${n(a.x)} ${n(a.y)}` +
		knotPath(a) +
		` C${n(a.x + a.tx * handle)} ${n(a.y + a.ty * handle)} ${n(b.x - b.tx * handle)} ${n(
			b.y - b.ty * handle
		)} ${n(b.x)} ${n(b.y)}`
	);
}

export type ScheduleOptions = {
	/** Largest reachable scroll offset. */
	maxScroll: number;
	/** Minimum scroll distance spent per pixel of thread. */
	pace: number;
	/** Scroll distance kept free after the last knot (the closing scene). */
	reserve: number;
};

export type Schedule = {
	/** Scroll offset at which Anda reaches each knot. */
	arrive: number[];
	/** Scroll offset at which Anda moves on; later than `arrive` when it stops to work. */
	leave: number[];
};

/**
 * When Anda reaches and leaves each knot. `stops[i]` is the preferred arrival
 * (the knot on the reading line), `dwells[i]` the scroll distance Anda stays
 * there, and `lengths[i]` the thread between knot i and i + 1. Arrivals never
 * run backwards, and the last knot is pulled in so the closing scene fits
 * before the page ends.
 */
export function scheduleKnots(
	stops: readonly number[],
	dwells: readonly number[],
	lengths: readonly number[],
	{ maxScroll, pace, reserve }: ScheduleOptions
): Schedule {
	const count = stops.length;
	const arrive = new Array<number>(count).fill(0);
	const leave = new Array<number>(count).fill(0);
	if (!count) return { arrive, leave };

	arrive[0] = stops[0];
	leave[0] = arrive[0] + dwells[0];
	for (let index = 1; index < count; index += 1) {
		arrive[index] = Math.max(stops[index], leave[index - 1] + pace * lengths[index - 1]);
		leave[index] = arrive[index] + dwells[index];
	}

	const last = count - 1;
	leave[last] = arrive[last];
	const limit = maxScroll - reserve;
	if (arrive[last] > limit) {
		arrive[last] = leave[last] = Math.max(limit, arrive[0]);
		for (let index = last - 1; index >= 0; index -= 1) {
			leave[index] = Math.min(leave[index], arrive[index + 1] - 1);
			arrive[index] = Math.min(arrive[index], leave[index] - dwells[index]);
		}
	}
	return { arrive, leave };
}

export type Keyframes = { times: number[]; lengths: number[] };

/** Scroll → drawn-length keyframes; a dwell is a flat stretch at its knot. */
export function keyframes({ arrive, leave }: Schedule, cumulative: readonly number[]): Keyframes {
	const times: number[] = [];
	const lengths: number[] = [];
	arrive.forEach((time, knot) => {
		times.push(time);
		lengths.push(cumulative[knot]);
		if (leave[knot] > time) {
			times.push(leave[knot]);
			lengths.push(cumulative[knot]);
		}
	});
	return { times, lengths };
}

/** Thread length drawn at scroll offset `scroll`. */
export function drawnAt(scroll: number, { times, lengths }: Keyframes): number {
	const count = times.length;
	if (!count) return 0;
	if (scroll <= times[0]) return lengths[0];
	for (let index = 0; index < count - 1; index += 1) {
		const from = times[index];
		const to = times[index + 1];
		if (scroll < to) {
			const t = to > from ? (scroll - from) / (to - from) : 1;
			return lengths[index] + (lengths[index + 1] - lengths[index]) * t;
		}
	}
	return lengths[count - 1];
}

export type Interlude = { top: number; height: number; target: number };

/**
 * Page theme (0 = night, 1 = paper). An interlude only changes the theme while
 * it covers the whole viewport, so no section is ever read mid-transition.
 */
export function themeAt(scroll: number, viewport: number, interludes: readonly Interlude[]) {
	let theme = 0;
	for (const interlude of interludes) {
		const range = interlude.height - viewport;
		const raw = range > 0 ? (scroll - interlude.top) / range : scroll >= interlude.top ? 1 : 0;
		const progress = smoothstep(0.12, 0.88, raw);
		if (progress <= 0) break;
		theme += (interlude.target - theme) * progress;
	}
	return theme;
}
