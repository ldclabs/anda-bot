import { clamp } from './geometry';

// Anda as a little flying sprite. It chases the journey point on a springy
// leash (so it overshoots and settles), leans into its flight, stretches when
// it zips and squashes when it lands, paddles its legs while travelling, and
// looks where it is going or at the pointer. At a knot it hovers beside the
// card on the free side and plays that knot's work pose. Tapping it makes it
// wobble like a roly-poly toy (and hop, while it is small), squint happily and
// pop an emote. In the closing scene it grows into the portrait.
// Markup and poses live in `AndaActor.svelte`.

export type ActorFrame = {
	/** Journey point, in root coordinates. */
	x: number;
	y: number;
	/** Work pose while resting at a knot, '' while travelling. */
	pose: string;
	/** Screen side (-1 left, 1 right, 0 none) to hover on while working. */
	side: number;
	/** 0..1 progress of the closing scene. */
	finale: number;
	/** Width the sprite grows to in the closing scene. */
	homeWidth: number;
	/** Pointer in root coordinates, or NaN when unknown. */
	pointerX: number;
	pointerY: number;
};

export type Actor = {
	measure(): void;
	/** A little celebration, e.g. when a memory is saved. */
	cheer(): void;
	/** Advances one frame; returns true while Anda is still moving. */
	update(frame: ActorFrame, dt: number): boolean;
	destroy(): void;
};

const EMOTES = ['heart', 'spark', 'note', 'star'];

/** `wake` asks the page for animation frames when Anda is poked. */
export function createActor(el: HTMLElement, wake: () => void): Actor {
	const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
	const body = el.querySelector<HTMLElement>('.body');
	let size = 72;
	let x = Number.NaN;
	let y = Number.NaN;
	let vx = 0;
	let vy = 0;
	let hoverX = 0;
	let squash = 0;
	let squashVelocity = 0;
	let wobble = 0;
	let wobbleVelocity = 0;
	let finale = 0;
	let phase = 0;
	let lookX = 0;
	let lookY = 0;
	let rest = 0;
	let lastWork = '';
	let pose = '';
	let emote = '';
	let emoteLeft = 0;

	function set(name: 'pose' | 'emote', value: string, previous: string) {
		if (value !== previous) el.dataset[name] = value;
		return value;
	}

	function burst() {
		if (reduced.matches) return;
		const sparks = el.querySelectorAll<HTMLElement>('.spark');
		sparks.forEach((spark, index) => {
			const angle = (index / sparks.length) * Math.PI * 2 + Math.random() * 0.6;
			// Spark sizes: fly 3–5 of them out, i.e. clear of the head.
			const reach = 300 + Math.random() * 180;
			spark.animate(
				[
					{ opacity: 1, transform: 'translate(-50%, -50%) scale(0.3)' },
					{
						opacity: 0,
						transform: `translate(calc(-50% + ${(Math.cos(angle) * reach).toFixed(0)}%), calc(-50% + ${(
							Math.sin(angle) * reach
						).toFixed(0)}%)) scale(1)`
					}
				],
				{ duration: 720, easing: 'cubic-bezier(0.2, 0.8, 0.3, 1)' }
			);
		});
	}

	function cheer() {
		emote = set('emote', EMOTES[Math.floor(Math.random() * EMOTES.length)], emote);
		emoteLeft = 1.4;
		vy -= 560 * (1 - finale);
		squashVelocity -= 3.5 * (1 - finale * 0.6);
		wobbleVelocity += (Math.random() < 0.5 ? -1 : 1) * 2.6;
		burst();
		wake();
	}

	function handleTap(event: PointerEvent) {
		event.preventDefault();
		cheer();
	}
	body?.addEventListener('pointerdown', handleTap);

	return {
		measure() {
			size = el.offsetWidth || size;
		},
		cheer,
		update(frame, dt) {
			const step = Math.min(dt, 1 / 30);
			finale = clamp(frame.finale);
			const grow = 1 + (frame.homeWidth / size - 1) * (finale * finale * (3 - 2 * finale));

			// Hover beside the card while working, never on top of its content.
			const wantHover = frame.side * size * 0.72 * (1 - finale);
			hoverX += (wantHover - hoverX) * (1 - Math.exp(-step * 5));
			const targetX = frame.x + hoverX;
			const targetY = frame.y;
			if (Number.isNaN(x)) {
				x = targetX;
				y = targetY;
			}

			// A slightly underdamped spring: Anda overshoots, then settles.
			const stiffness = 70 + finale * 140;
			const damping = 2 * Math.sqrt(stiffness) * 0.72;
			for (let pass = 0; pass < 2; pass += 1) {
				const h = step / 2;
				vx += (stiffness * (targetX - x) - damping * vx) * h;
				vy += (stiffness * (targetY - y) - damping * vy) * h;
				x += vx * h;
				y += vy * h;
			}
			const speed = Math.hypot(vx, vy);

			// Land with a squash when work starts; stretch along fast flight.
			if (frame.pose && frame.pose !== lastWork) {
				squashVelocity += 4;
				burst();
			}
			lastWork = frame.pose;
			squashVelocity += (-160 * squash - 14 * squashVelocity) * step;
			squash = clamp(squash + squashVelocity * step, -0.35, 0.35);
			const stretch = clamp(speed / 2400, 0, 0.16);
			const vertical = speed > 1 ? Math.abs(vy) / speed : 0;
			const scaleY = (1 - squash) * (1 + stretch * (vertical - 0.4));
			const scaleX = (1 + squash * 0.6) * (1 - stretch * (vertical - 0.4) * 0.6);

			// Lean into the flight; while working, tip toward the card.
			rest += (frame.side * -0.12 - rest) * (1 - Math.exp(-step * 4));
			const lean = (clamp(vx * 0.00032, -0.32, 0.32) + rest) * (1 - finale);

			// Tap: a damped roly-poly wobble about the feet (see .body in the markup).
			wobbleVelocity += (-110 * wobble - 4.2 * wobbleVelocity) * step;
			wobble += wobbleVelocity * step;

			const travelling = clamp(speed / 500) * (1 - finale);
			if (travelling > 0.03) phase += step * (8 + travelling * 10);

			// Look where it flies, or at the pointer while it rests.
			const dx = frame.pointerX - x;
			const dy = frame.pointerY - y;
			const pointerKnown = !Number.isNaN(dx);
			let wantX = 0;
			let wantY = 0;
			if (travelling > 0.15 && speed > 1) {
				wantX = (vx / speed) * 0.9;
				wantY = (vy / speed) * 0.7;
			} else if (pointerKnown) {
				wantX = clamp(dx / 380, -1, 1);
				wantY = clamp(dy / 300, -1, 1);
			}
			lookX += (wantX - lookX) * (1 - Math.exp(-step * 10));
			lookY += (wantY - lookY) * (1 - Math.exp(-step * 10));
			const near = pointerKnown && Math.hypot(dx, dy) < size * grow * 1.6;

			if (emoteLeft > 0) {
				emoteLeft -= step;
				if (emoteLeft <= 0) emote = set('emote', '', emote);
			}

			el.style.transform =
				`translate(${x.toFixed(1)}px, ${y.toFixed(1)}px) rotate(${lean.toFixed(4)}rad) ` +
				`scale(${(scaleX * grow).toFixed(4)}, ${(scaleY * grow).toFixed(4)}) ` +
				`translate(${(-size / 2).toFixed(1)}px, ${(-size / 2).toFixed(1)}px)`;
			el.style.setProperty('--step', (Math.sin(phase) * travelling).toFixed(3));
			el.style.setProperty('--look-x', lookX.toFixed(3));
			el.style.setProperty('--look-y', lookY.toFixed(3));
			el.style.setProperty('--grow', grow.toFixed(3));
			el.style.setProperty('--finale', finale.toFixed(3));
			el.style.setProperty('--wobble', wobble.toFixed(4));

			pose = set(
				'pose',
				finale >= 0.999
					? 'home'
					: frame.pose || (travelling > 0.15 ? 'fly' : near ? 'curious' : 'hover'),
				pose
			);

			return (
				speed > 2 ||
				Math.abs(targetX - x) + Math.abs(targetY - y) > 0.5 ||
				Math.abs(squash) + Math.abs(squashVelocity) > 0.004 ||
				Math.abs(wantHover - hoverX) > 0.5 ||
				Math.abs(wantX - lookX) + Math.abs(wantY - lookY) > 0.01 ||
				Math.abs(wobble) + Math.abs(wobbleVelocity) > 0.002 ||
				emoteLeft > 0
			);
		},
		destroy() {
			body?.removeEventListener('pointerdown', handleTap);
		}
	};
}
