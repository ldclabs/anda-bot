import { describe, expect, it } from 'vitest';
import { drawnAt, keyframes, knotPath, scheduleKnots, segmentPath, themeAt } from './geometry';

const down = { tx: 0, ty: 1, loop: 0 };

describe('segmentPath', () => {
	it('runs from anchor to anchor with tangent handles', () => {
		const d = segmentPath({ x: 10, y: 0, ...down }, { x: 10, y: 200, ...down });
		expect(d.startsWith('M10 0')).toBe(true);
		expect(d.endsWith('10 200')).toBe(true);
		expect(d.match(/C/g)).toHaveLength(1);
	});

	it('ties a closed knot that leaves along the tangent', () => {
		const knot = knotPath({ x: 0, y: 0, tx: 0, ty: 1, loop: 10 });
		expect(knot.match(/C/g)).toHaveLength(4);
		// the knot returns to the anchor it started from
		expect(knot.trim().endsWith('0 0')).toBe(true);
		expect(knotPath({ x: 0, y: 0, ...down })).toBe('');
	});
});

describe('scheduleKnots', () => {
	const options = { maxScroll: 2000, pace: 0.5, reserve: 0 };

	it('keeps anchored stops and never runs backwards', () => {
		const { arrive } = scheduleKnots([-100, 300, 250, 900], [0, 0, 0, 0], [200, 100, 400], options);
		expect(arrive).toEqual([-100, 300, 350, 900]);
	});

	it('lets Anda stay at a knot before moving on', () => {
		const { arrive, leave } = scheduleKnots([0, 100, 400], [0, 150, 0], [100, 100], options);
		expect(arrive[1]).toBe(100);
		expect(leave[1]).toBe(250);
		// the next knot waits for the dwell plus its share of thread
		expect(arrive[2]).toBe(400);
		const late = scheduleKnots([0, 100, 200], [0, 150, 0], [100, 100], options);
		expect(late.arrive[2]).toBe(300);
	});

	it('keeps the closing scene inside the page', () => {
		const { arrive, leave } = scheduleKnots([0, 2600, 2700], [0, 40, 0], [10, 10], {
			...options,
			reserve: 300
		});
		expect(arrive[2]).toBe(1700);
		expect(leave[1]).toBeLessThan(arrive[2]);
		expect(arrive[1]).toBeLessThanOrEqual(leave[1] - 40);
	});
});

describe('drawnAt', () => {
	const timeline = keyframes({ arrive: [-50, 100, 300], leave: [-50, 180, 300] }, [0, 400, 600]);

	it('interpolates thread length and holds it while Anda dwells', () => {
		expect(timeline.times).toEqual([-50, 100, 180, 300]);
		expect(drawnAt(-80, timeline)).toBe(0);
		expect(drawnAt(0, timeline)).toBeCloseTo((50 / 150) * 400);
		expect(drawnAt(140, timeline)).toBe(400);
		expect(drawnAt(240, timeline)).toBe(500);
		expect(drawnAt(900, timeline)).toBe(600);
	});
});

describe('themeAt', () => {
	const interludes = [
		{ top: 1000, height: 2000, target: 1 },
		{ top: 6000, height: 2000, target: 0 }
	];

	it('only changes while an interlude fills the viewport', () => {
		expect(themeAt(900, 1000, interludes)).toBe(0);
		expect(themeAt(1500, 1000, interludes)).toBeCloseTo(0.5);
		expect(themeAt(2000, 1000, interludes)).toBe(1);
		expect(themeAt(5000, 1000, interludes)).toBe(1);
		expect(themeAt(7000, 1000, interludes)).toBe(0);
	});
});
