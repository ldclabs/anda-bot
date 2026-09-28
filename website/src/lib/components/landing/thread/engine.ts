import { createActor } from './actor';
import {
	clamp,
	drawnAt,
	keyframes,
	scheduleKnots,
	segmentPath,
	themeAt,
	type Interlude,
	type Keyframes,
	type ThreadAnchor
} from './geometry';

// Anda's journey through the landing page. Anchors are zero-size
// `[data-thread]` markers placed by CSS; Anda flies through them in order,
// keeping pace with the reader's scroll position and leaving a faint trail of
// dots. At an anchor with `data-thread-dwell` it stays for that much scroll
// and plays `data-thread-pose` beside the card (`data-thread-side`).
// `[data-station]` elements receive `--arrive` (0..1) while Anda works at, or
// travels on from, the anchor naming that station. The journey ends at
// `[data-actor-home]`, where Anda grows into the portrait.

const SVG_NS = 'http://www.w3.org/2000/svg';
/** Distance between trail dots, in px. */
const DOT_GAP = 15;
/** Distance between samples of the journey lookup table, in px. */
const TRACK_STEP = 4;

type Station = { name: string; from: number; to: number };
type Tracked = { el: HTMLElement; top: number; height: number; value: number };

export type ThreadEngine = {
	/** Re-measure anchors after content or layout changes. */
	refresh(): void;
	/** Anda celebrates (e.g. a memory was just saved). */
	cheer(): void;
	destroy(): void;
};

export type ThreadHooks = {
	onSection?: (id: string) => void;
};

function setVar(el: HTMLElement, name: string, value: number, previous: number, epsilon = 0.001) {
	if (Math.abs(value - previous) < epsilon) return previous;
	el.style.setProperty(name, value.toFixed(4));
	return value;
}

export function createThread(
	root: HTMLElement,
	svg: SVGSVGElement,
	actorEl: HTMLElement,
	hooks: ThreadHooks = {}
): ThreadEngine {
	const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
	const actor = createActor(actorEl, () => request());
	// An unrendered path used to measure each stretch of the journey. It is
	// only read while measuring; frames interpolate the sampled track instead.
	const route = document.createElementNS(SVG_NS, 'path');
	route.setAttribute('visibility', 'hidden');
	const trail = document.createElementNS(SVG_NS, 'g');
	trail.setAttribute('class', 't-trail');
	svg.replaceChildren(route, trail);

	let timeline: Keyframes = { times: [], lengths: [] };
	let arrive: number[] = [];
	let leave: number[] = [];
	let poses: string[] = [];
	/** Screen side (-1 left, 1 right) Anda hovers on while working at a knot. */
	let sides: number[] = [];
	/** Journey length at the start of each stretch (and the total at the end). */
	let starts: number[] = [0];
	let total = 0;
	/** Journey position every TRACK_STEP px, so frames never touch SVG geometry. */
	let trackX = new Float32Array(0);
	let trackY = new Float32Array(0);
	let dots: { el: SVGCircleElement; at: number }[] = [];
	let lit = 0;
	let homeWidth = 0;
	let stationEls: { el: HTMLElement; station: Station; value: number; live: boolean }[] = [];
	let interludes: (Interlude & { el: HTMLElement; value: number })[] = [];
	let reveals: Tracked[] = [];
	let heroes: Tracked[] = [];
	let navSections: { id: string; top: number }[] = [];
	let rootLeft = 0;
	/** Document offset of the root's top edge. */
	let docTop = 0;
	let viewport = 0;
	let maxScroll = 1;
	/** Last scroll offset, read in the scroll event: reading it inside a frame,
	 * after style writes, would force a synchronous layout. */
	let scrollTop = window.scrollY;
	/** Smoothed scroll position that drives the whole journey. */
	let shown = 0;
	let drawn = 0;
	let measured = false;
	let ready = false;
	let theme = -1;
	let scrolled = -1;
	let page = -1;
	// Scroll-linked variables go only to the elements that use them, so a
	// frame never restyles the whole document.
	let headerEl: HTMLElement | null = null;
	let progressEl: HTMLElement | null = null;
	let themed: HTMLElement[] = [];
	let activeSection = '';
	let raf = 0;
	let lastFrame = 0;
	let reduced = motionQuery.matches;
	let pointerX = Number.NaN;
	let pointerY = Number.NaN;
	let destroyed = false;

	function anchorFrom(el: HTMLElement, rtl: boolean, rootTop: number): ThreadAnchor {
		const rect = el.getBoundingClientRect();
		const dir = el.dataset.threadDir ?? 'v';
		let tx = 0;
		let ty = 1;
		if (dir === 'h' || dir === '-h') {
			tx = (dir === 'h' ? 1 : -1) * (rtl ? -1 : 1);
			ty = 0;
		} else if (dir === 'up') {
			ty = -1;
		}
		return {
			x: rect.left + rect.width / 2 - rootLeft,
			y: rect.top + rect.height / 2 - rootTop,
			tx,
			ty,
			loop: Number(el.dataset.threadLoop ?? 0) * (rtl ? -1 : 1)
		};
	}

	function measure() {
		if (destroyed) return;
		const rootRect = root.getBoundingClientRect();
		const rootTop = rootRect.top;
		rootLeft = rootRect.left;
		const scroll = (scrollTop = window.scrollY);
		viewport = window.innerHeight;
		maxScroll = Math.max(1, document.documentElement.scrollHeight - viewport);
		const rtl = getComputedStyle(root).direction === 'rtl';
		const head = viewport * (window.innerWidth < 768 ? 0.62 : 0.55);
		docTop = rootTop + scroll;
		actor.measure();

		svg.setAttribute('width', String(Math.ceil(rootRect.width)));
		svg.setAttribute('height', String(Math.ceil(rootRect.height)));

		const markers = [...root.querySelectorAll<HTMLElement>('[data-thread]')]
			.filter((el) => el.getClientRects().length > 0)
			.sort((a, b) => Number(a.dataset.thread) - Number(b.dataset.thread));
		const anchors = markers.map((el) => anchorFrom(el, rtl, rootTop));
		const dwells = markers.map((el) => Number(el.dataset.threadDwell ?? 0));
		poses = markers.map((el) => el.dataset.threadPose ?? '');
		// Single-column layouts keep Anda in the margin lane instead.
		const sideScale = window.innerWidth < 1024 ? 0 : rtl ? -1 : 1;
		sides = markers.map((el) => {
			const side = el.dataset.threadSide;
			return (side === 'start' ? -1 : side === 'end' ? 1 : 0) * sideScale;
		});

		// The journey ends in the middle of the portrait box, where Anda grows.
		const home = root.querySelector<HTMLElement>('[data-actor-home]');
		homeWidth = 0;
		if (home?.getClientRects().length) {
			const rect = home.getBoundingClientRect();
			homeWidth = rect.width;
			anchors.push({
				x: rect.left + rect.width / 2 - rootLeft,
				y: rect.top + rect.height / 2 - rootTop,
				tx: 0,
				ty: 1,
				loop: 0
			});
			dwells.push(0);
			poses.push('');
			sides.push(0);
		}

		// Lay out each stretch of the journey: sample it into the lookup track
		// and place the trail dots along it.
		starts = [0];
		dots = [];
		const xs: number[] = [];
		const ys: number[] = [];
		let nextSample = 0;
		for (let index = 0; index < anchors.length - 1; index += 1) {
			route.setAttribute('d', segmentPath(anchors[index], anchors[index + 1]));
			const length = route.getTotalLength();
			const start = starts[starts.length - 1];
			for (; nextSample <= start + length; nextSample += TRACK_STEP) {
				const point = route.getPointAtLength(nextSample - start);
				xs.push(point.x);
				ys.push(point.y);
			}
			for (let at = DOT_GAP - (start % DOT_GAP); at < length; at += DOT_GAP) {
				const point = route.getPointAtLength(at);
				const el = document.createElementNS(SVG_NS, 'circle');
				el.setAttribute('cx', point.x.toFixed(1));
				el.setAttribute('cy', point.y.toFixed(1));
				el.setAttribute('r', '1.8');
				dots.push({ el, at: start + at });
			}
			starts.push(start + length);
		}
		total = starts[starts.length - 1];
		if (xs.length) {
			const end = route.getPointAtLength(route.getTotalLength());
			xs.push(end.x);
			ys.push(end.y);
		}
		trackX = Float32Array.from(xs);
		trackY = Float32Array.from(ys);
		headerEl = root.querySelector<HTMLElement>('[data-scrolled]');
		themed = [...root.querySelectorAll<HTMLElement>('[data-themed]')];
		progressEl = root.querySelector<HTMLElement>('[data-progress]');
		scrolled = page = -1;
		lit = 0;
		trail.replaceChildren(...dots.map((dot) => dot.el));

		// Anda works at an anchor while it crosses the reading line (the dwell
		// is centred on that moment); anchors high on the first screen are
		// reached as soon as the page opens.
		const stops = anchors.map(({ y }, index) => {
			const at = y + docTop;
			return at < viewport * 0.75 ? at - viewport : at - head - dwells[index] / 2;
		});
		({ arrive, leave } = scheduleKnots(
			stops,
			dwells,
			starts.slice(1).map((end, index) => end - starts[index]),
			{ maxScroll, pace: 0.2, reserve: homeWidth ? viewport * 0.45 : 0 }
		));
		timeline = keyframes({ arrive, leave }, starts);

		// A station runs while Anda works at its anchor, or until the next one.
		const stations = new Map<string, Station>();
		markers.forEach((el, index) => {
			const name = el.dataset.threadStation;
			if (!name) return;
			const to = leave[index] > arrive[index] ? leave[index] : arrive[index + 1];
			stations.set(name, { name, from: arrive[index], to: to ?? arrive[index] + 1 });
		});
		if (homeWidth) {
			const last = arrive.length - 1;
			stations.set('finale', { name: 'finale', from: arrive[last], to: maxScroll });
		}
		stationEls = [...root.querySelectorAll<HTMLElement>('[data-station]')].flatMap((el) => {
			const station = stations.get(el.dataset.station ?? '');
			return station ? [{ el, station, value: -1, live: false }] : [];
		});

		const track = (el: HTMLElement): Tracked => {
			const rect = el.getBoundingClientRect();
			return { el, top: rect.top + scroll, height: rect.height, value: -1 };
		};
		interludes = [...root.querySelectorAll<HTMLElement>('[data-interlude]')].map((el) => ({
			...track(el),
			target: el.dataset.interlude === 'paper' ? 1 : 0
		}));
		reveals = [...root.querySelectorAll<HTMLElement>('[data-reveal]')].map(track);
		heroes = [...root.querySelectorAll<HTMLElement>('[data-hero]')].map(track);
		navSections = [...root.querySelectorAll<HTMLElement>('[data-nav]')].map((el) => ({
			id: el.id,
			top: el.getBoundingClientRect().top + scroll
		}));

		// On first load near the top Anda flies out of the chat and down to the
		// reading line; later re-measures pick up where the reader is.
		if (!measured) shown = scroll < viewport ? Math.min(scroll, arrive[0] ?? 0) : scroll;
		measured = true;
		theme = -1;
		root.classList.toggle('thread-reduced', reduced);
		request();
	}

	function pointOn(target: number) {
		const count = trackX.length;
		if (count < 2) return null;
		const at = clamp(target, 0, total) / TRACK_STEP;
		const index = Math.min(Math.floor(at), count - 2);
		const t = Math.min(1, at - index);
		return {
			x: trackX[index] + (trackX[index + 1] - trackX[index]) * t,
			y: trackY[index] + (trackY[index + 1] - trackY[index]) * t
		};
	}

	/** Lights trail dots up to the drawn length; fresh dots sparkle, then fade. */
	function paintTrail() {
		while (lit < dots.length && dots[lit].at <= drawn) {
			dots[lit].el.classList.add('on');
			lit += 1;
		}
		while (lit > 0 && dots[lit - 1].at > drawn) {
			lit -= 1;
			dots[lit].el.classList.remove('on');
		}
	}

	function paintActor(dt: number) {
		if (reduced) return false;
		const point = pointOn(drawn);
		if (!point) return false;
		let pose = '';
		let side = 0;
		for (let knot = 0; knot < arrive.length; knot += 1) {
			if (shown >= arrive[knot] && shown < leave[knot]) {
				pose = poses[knot];
				side = sides[knot];
				break;
			}
		}
		const last = arrive.length - 1;
		return actor.update(
			{
				x: point.x,
				y: point.y,
				pose,
				side,
				finale: homeWidth
					? clamp((shown - arrive[last]) / Math.max(1, maxScroll - arrive[last]))
					: 0,
				homeWidth: homeWidth || actorEl.offsetWidth,
				pointerX: pointerX - rootLeft,
				pointerY: pointerY + scrollTop - docTop
			},
			dt
		);
	}

	function paintStations() {
		for (const item of stationEls) {
			const { from, to } = item.station;
			const value = reduced ? 1 : clamp((shown - from) / Math.max(1, to - from));
			item.value = setVar(item.el, '--arrive', value, item.value);
			const live = item.live ? value > 0.4 : value > 0.985;
			if (live !== item.live) {
				item.live = live;
				item.el.toggleAttribute('data-live', live);
			}
		}
	}

	function paintScroll(scroll: number) {
		const nextTheme = themeAt(scroll, viewport, interludes);
		if (Math.abs(nextTheme - theme) >= 0.0005) {
			theme = nextTheme;
			for (const el of themed) el.style.setProperty('--theme', theme.toFixed(4));
		}
		if (headerEl) scrolled = setVar(headerEl, '--scrolled', clamp(scroll / 120), scrolled);
		if (progressEl) page = setVar(progressEl, '--page', clamp(scroll / maxScroll), page);
		for (const interlude of interludes) {
			const value = clamp((scroll + viewport - interlude.top) / (interlude.height + viewport));
			interlude.value = setVar(interlude.el, '--ip', value, interlude.value, 0.0005);
		}
		for (const item of reveals) {
			const value = reduced ? 1 : clamp((scroll + viewport - item.top) / (viewport * 0.55));
			item.value = setVar(item.el, '--enter', value, item.value);
		}
		for (const item of heroes) {
			const value = reduced ? 0 : clamp(scroll / Math.max(1, item.height * 0.85));
			item.value = setVar(item.el, '--hp', value, item.value);
		}
		let section = '';
		for (const item of navSections) {
			if (item.top <= scroll + viewport * 0.45) section = item.id;
		}
		if (section !== activeSection) {
			activeSection = section;
			hooks.onSection?.(section);
		}
	}

	function frame(now: number) {
		raf = 0;
		if (destroyed || !measured) return;
		const dt = lastFrame ? Math.min(0.05, (now - lastFrame) / 1000) : 1 / 60;
		lastFrame = now;
		const scroll = scrollTop;
		let busy = false;

		const gap = scroll - shown;
		if (Math.abs(gap) > 0.3 && !reduced) {
			shown += gap * (1 - Math.exp(-dt * 10));
			busy = true;
		} else {
			shown = scroll;
		}
		drawn = reduced ? total : drawnAt(shown, timeline);

		paintTrail();
		paintStations();
		paintScroll(scroll);
		if (paintActor(dt)) busy = true;

		if (!ready) {
			ready = true;
			root.classList.add('thread-ready');
		}
		if (busy) request();
		else lastFrame = 0;
	}

	function request() {
		if (!raf && !destroyed) raf = requestAnimationFrame(frame);
	}

	let measureQueued = 0;
	function queueMeasure() {
		if (measureQueued) return;
		measureQueued = requestAnimationFrame(() => {
			measureQueued = 0;
			measure();
		});
	}

	function handlePointer(event: PointerEvent) {
		pointerX = event.clientX;
		pointerY = event.clientY;
		request();
	}

	function handleMotion() {
		reduced = motionQuery.matches;
		queueMeasure();
	}

	const resizeObserver = new ResizeObserver(() => queueMeasure());
	resizeObserver.observe(root);
	function handleScroll() {
		scrollTop = window.scrollY;
		request();
	}

	window.addEventListener('scroll', handleScroll, { passive: true });
	window.addEventListener('resize', queueMeasure, { passive: true });
	window.addEventListener('pointermove', handlePointer, { passive: true });
	motionQuery.addEventListener('change', handleMotion);
	void document.fonts?.ready.then(() => queueMeasure());
	measure();

	return {
		refresh: queueMeasure,
		cheer() {
			// Only while the reader can see both the chat and Anda.
			if (!reduced && measured && scrollTop < viewport * 0.6) actor.cheer();
		},
		destroy() {
			destroyed = true;
			cancelAnimationFrame(raf);
			cancelAnimationFrame(measureQueued);
			resizeObserver.disconnect();
			window.removeEventListener('scroll', handleScroll);
			window.removeEventListener('resize', queueMeasure);
			window.removeEventListener('pointermove', handlePointer);
			motionQuery.removeEventListener('change', handleMotion);
			actor.destroy();
			svg.replaceChildren();
		}
	};
}
