<script lang="ts">
	import { onMount } from 'svelte';

	type MemoryNode = {
		x: number;
		y: number;
		homeX: number;
		homeY: number;
		vx: number;
		vy: number;
		radius: number;
		phase: number;
		weight: number;
		tone: number;
		label: string;
	};

	type MemoryStream = {
		from: number;
		to: number;
		bend: number;
		tone: number;
		delay: number;
	};

	let canvas: HTMLCanvasElement;

	const labels = [
		'identity',
		'projects',
		'preferences',
		'decisions',
		'timelines',
		'tools',
		'channels',
		'files',
		'recall',
		'formation',
		'sleep',
		'actions',
		'rituals',
		'sources'
	];

	onMount(() => {
		const maybeContext = canvas.getContext('2d', { alpha: false });
		if (!maybeContext) return;

		const context: CanvasRenderingContext2D = maybeContext as CanvasRenderingContext2D;
		const palette = ['#f1a64e', '#35b2ab', '#d84b42', '#ffd08a', '#a9c9a3'];
		const pointer = { x: 0, y: 0, active: false };
		const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');

		let width = 0;
		let height = 0;
		let frame = 0;
		let animation = 0;
		let reducedMotion = motionQuery.matches;
		let nodes: MemoryNode[] = [];
		let streams: MemoryStream[] = [];
		let backdrop: HTMLCanvasElement | null = null;
		let backdropCtx: CanvasRenderingContext2D | null = null;
		let lastResizeW = 0;
		let lastResizeH = 0;
		let lastFrameTime = 0;
		const targetFrameMs = 1000 / 40; // cap ~40fps

		function seed(index: number) {
			const value = Math.sin(index * 127.1 + 311.7) * 43758.5453123;
			return value - Math.floor(value);
		}

		function buildField() {
			const total = width < 640 ? 32 : width < 1024 ? 44 : 56;
			const centerX = width * (width < 900 ? 0.54 : 0.66);
			const centerY = height * 0.48;
			const scaleX = width * (width < 640 ? 0.34 : 0.28);
			const scaleY = height * 0.26;

			nodes = Array.from({ length: total }, (_, index) => {
				const progress = index / total;
				const angle = progress * Math.PI * 2 + seed(index) * 0.42;
				const fold = 1 + Math.sin(angle * 2.35 + 0.8) * 0.18 + (seed(index + 9) - 0.5) * 0.18;
				const innerPull = index % 9 === 0 ? 0.45 : 1;
				const homeX = centerX + Math.cos(angle) * scaleX * fold * innerPull + Math.sin(angle * 3.1) * scaleX * 0.08;
				const homeY = centerY + Math.sin(angle) * scaleY * (0.68 + seed(index + 13) * 0.26) * innerPull;

				return {
					x: homeX,
					y: homeY,
					homeX,
					homeY,
					vx: (seed(index + 21) - 0.5) * 0.16,
					vy: (seed(index + 33) - 0.5) * 0.16,
					radius: 1.5 + seed(index + 45) * 3.6,
					phase: seed(index + 57) * Math.PI * 2,
					weight: 0.45 + seed(index + 69) * 0.9,
					tone: index % palette.length,
					label: labels[index % labels.length]
				};
			});

			streams = Array.from({ length: Math.floor(total * 1.1) }, (_, index) => {
				const from = Math.floor(seed(index + 101) * total);
				const hop = 4 + Math.floor(seed(index + 113) * 18);
				return {
					from,
					to: (from + hop) % total,
					bend: (seed(index + 127) - 0.5) * Math.min(width, height) * 0.42,
					tone: index % palette.length,
					delay: seed(index + 139)
				};
			});
		}

		function renderBackdrop() {
			if (!backdrop || !backdropCtx) return;
			const bctx = backdropCtx;
			bctx.clearRect(0, 0, width, height);

			const base = bctx.createLinearGradient(0, 0, width, height);
			base.addColorStop(0, '#06100e');
			base.addColorStop(0.38, '#0e1c18');
			base.addColorStop(0.7, '#1c241d');
			base.addColorStop(1, '#070f0d');
			bctx.fillStyle = base;
			bctx.fillRect(0, 0, width, height);

			// static contour rings (no animation)
			const centerX = width * (width < 900 ? 0.54 : 0.66);
			const centerY = height * 0.48;
			for (let layer = 0; layer < 7; layer += 1) {
				const scale = 0.58 + layer * 0.09;
				bctx.beginPath();
				for (let step = 0; step <= 120; step += 1) {
					const angle = (step / 120) * Math.PI * 2;
					const wave = Math.sin(angle * 2.4 + layer) * 0.12 + Math.cos(angle * 5.1) * 0.05;
					const x = centerX + Math.cos(angle) * width * 0.23 * scale * (1 + wave);
					const y = centerY + Math.sin(angle) * height * 0.22 * scale * (0.7 + wave * 0.4);
					if (step === 0) bctx.moveTo(x, y);
					else bctx.lineTo(x, y);
				}
				bctx.closePath();
				bctx.strokeStyle = `rgba(244, 238, 227, ${0.035 + layer * 0.009})`;
				bctx.lineWidth = 1;
				bctx.stroke();
			}

			// scanlines baked once
			bctx.fillStyle = 'rgba(244, 238, 227, 0.045)';
			for (let y = 0; y < height; y += 5) {
				bctx.fillRect(0, y, width, 1);
			}
		}

		function resize() {
			const rect = canvas.getBoundingClientRect();
			const devicePixelRatio = Math.min(window.devicePixelRatio || 1, 1.5);

			const newWidth = Math.max(rect.width, 320);
			const newHeight = Math.max(rect.height, 460);

			// Skip rebuild on negligible size changes (avoids ResizeObserver thrash)
			if (Math.abs(newWidth - lastResizeW) < 2 && Math.abs(newHeight - lastResizeH) < 2 && nodes.length) {
				return;
			}
			lastResizeW = newWidth;
			lastResizeH = newHeight;
			width = newWidth;
			height = newHeight;

			canvas.width = Math.floor(width * devicePixelRatio);
			canvas.height = Math.floor(height * devicePixelRatio);
			context.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);

			if (!backdrop) {
				backdrop = document.createElement('canvas');
				backdropCtx = backdrop.getContext('2d');
			}
			if (backdrop && backdropCtx) {
				backdrop.width = Math.floor(width * devicePixelRatio);
				backdrop.height = Math.floor(height * devicePixelRatio);
				backdropCtx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
			}

			buildField();
			renderBackdrop();
		}

		function quadraticPoint(start: MemoryNode, end: MemoryNode, bend: number, progress: number) {
			const midX = (start.x + end.x) / 2 + bend * 0.34;
			const midY = (start.y + end.y) / 2 - bend * 0.18;
			const inverse = 1 - progress;

			return {
				x: inverse * inverse * start.x + 2 * inverse * progress * midX + progress * progress * end.x,
				y: inverse * inverse * start.y + 2 * inverse * progress * midY + progress * progress * end.y
			};
		}

		function draw(now?: number) {
			if (!reducedMotion) animation = requestAnimationFrame(draw);

			// Frame throttle to ~40fps to reduce CPU load
			const time = now ?? performance.now();
			if (time - lastFrameTime < targetFrameMs) return;
			lastFrameTime = time;

			frame += reducedMotion ? 0.2 : 1;

			// Blit cached backdrop instead of redrawing gradient + contours + scanlines
			if (backdrop) {
				context.drawImage(backdrop, 0, 0, width, height);
			} else {
				context.fillStyle = '#0e1c18';
				context.fillRect(0, 0, width, height);
			}

			context.save();
			context.globalCompositeOperation = 'lighter';

			for (const node of nodes) {
				const homePullX = node.homeX - node.x;
				const homePullY = node.homeY - node.y;
				node.vx += homePullX * 0.00055 + Math.sin(frame * 0.012 + node.phase) * 0.002;
				node.vy += homePullY * 0.00055 + Math.cos(frame * 0.011 + node.phase) * 0.002;

				if (pointer.active) {
					const dx = node.x - pointer.x;
					const dy = node.y - pointer.y;
					const distance = Math.hypot(dx, dy) || 1;
					if (distance < 260) {
						const force = (1 - distance / 260) * 0.045;
						node.vx += (dx / distance) * force;
						node.vy += (dy / distance) * force;
					}
				}

				node.vx *= 0.965;
				node.vy *= 0.965;
				node.x += node.vx;
				node.y += node.vy;
			}

			// Draw streams grouped by color/dash to minimize state changes
			context.setLineDash([2, 12]);
			context.lineDashOffset = -frame * 0.32;
			for (const stream of streams) {
				const start = nodes[stream.from];
				const end = nodes[stream.to];
				if (!start || !end) continue;

				const alpha = 0.08 + Math.sin(frame * 0.012 + stream.delay * 8) * 0.025;
				const midX = (start.x + end.x) / 2 + stream.bend * 0.34;
				const midY = (start.y + end.y) / 2 - stream.bend * 0.18;

				context.strokeStyle = `${palette[stream.tone]}${Math.round(alpha * 255)
					.toString(16)
					.padStart(2, '0')}`;
				context.lineWidth = 0.7 + stream.delay * 1.4;
				context.beginPath();
				context.moveTo(start.x, start.y);
				context.quadraticCurveTo(midX, midY, end.x, end.y);
				context.stroke();

				if (!reducedMotion && stream.from % 5 === 0) {
					const progress = (frame * 0.0032 + stream.delay) % 1;
					const pulse = quadraticPoint(start, end, stream.bend, progress);
					context.fillStyle = palette[stream.tone];
					context.beginPath();
					context.arc(pulse.x, pulse.y, 1.8 + stream.delay * 2.2, 0, Math.PI * 2);
					context.fill();
				}
			}
			context.setLineDash([]);

			// Neighbor links: spatial bucket to avoid O(n^2)
			const cellSize = 110;
			const cols = Math.max(1, Math.ceil(width / cellSize));
			const buckets = new Map<number, number[]>();
			for (let i = 0; i < nodes.length; i += 1) {
				const n = nodes[i];
				const cx = Math.floor(n.x / cellSize);
				const cy = Math.floor(n.y / cellSize);
				const key = cy * cols + cx;
				let arr = buckets.get(key);
				if (!arr) {
					arr = [];
					buckets.set(key, arr);
				}
				arr.push(i);
			}
			context.lineWidth = 0.55;
			for (let i = 0; i < nodes.length; i += 1) {
				const a = nodes[i];
				const cx = Math.floor(a.x / cellSize);
				const cy = Math.floor(a.y / cellSize);
				for (let oy = 0; oy <= 1; oy += 1) {
					for (let ox = -1; ox <= 1; ox += 1) {
						if (oy === 0 && ox < 0) continue;
						const key = (cy + oy) * cols + (cx + ox);
						const bucket = buckets.get(key);
						if (!bucket) continue;
						for (const j of bucket) {
							if (j <= i) continue;
							const b = nodes[j];
							const dx = a.x - b.x;
							const dy = a.y - b.y;
							const threshold = 84 + (a.weight + b.weight) * 22;
							if (Math.abs(dx) > threshold || Math.abs(dy) > threshold) continue;
							const distance = Math.hypot(dx, dy);
							if (distance < threshold) {
								const alpha = (1 - distance / threshold) * 0.14;
								context.strokeStyle = `rgba(244,238,227,${alpha})`;
								context.beginPath();
								context.moveTo(a.x, a.y);
								context.lineTo(b.x, b.y);
								context.stroke();
							}
						}
					}
				}
			}

			// Nodes: avoid shadowBlur (very expensive). Use cheap halo ring instead.
			for (let index = 0; index < nodes.length; index += 1) {
				const node = nodes[index];
				const pulse = 1 + Math.sin(frame * 0.032 + node.phase) * 0.2;
				const color = palette[node.tone];

				context.fillStyle = color;
				context.beginPath();
				context.arc(node.x, node.y, node.radius * pulse, 0, Math.PI * 2);
				context.fill();

				context.strokeStyle = `rgba(244,238,227,${0.14 + node.weight * 0.08})`;
				context.lineWidth = 0.7;
				context.beginPath();
				context.arc(node.x, node.y, node.radius * 2.7, 0, Math.PI * 2);
				context.stroke();

				if (width > 900 && index % 12 === 0) {
					context.font = '12px Inter, sans-serif';
					context.fillStyle = 'rgba(244, 238, 227, 0.58)';
					context.fillText(node.label, node.x + 11, node.y - 9);
				}
			}

			context.restore();
		}

		function handlePointer(event: PointerEvent) {
			const rect = canvas.getBoundingClientRect();
			pointer.x = event.clientX - rect.left;
			pointer.y = event.clientY - rect.top;
			pointer.active = true;
		}

		function deactivatePointer() {
			pointer.active = false;
		}

		function handleMotionChange() {
			reducedMotion = motionQuery.matches;
			cancelAnimationFrame(animation);
			animation = 0;
			draw();
		}

		let visible = true;
		let docVisible = !document.hidden;

		function startLoop() {
			if (animation || !visible || !docVisible) return;
			lastFrameTime = 0;
			animation = requestAnimationFrame(draw);
		}

		function stopLoop() {
			if (animation) {
				cancelAnimationFrame(animation);
				animation = 0;
			}
		}

		function handleVisibilityChange() {
			docVisible = !document.hidden;
			if (docVisible) startLoop();
			else stopLoop();
		}

		resize();
		startLoop();

		const resizeObserver = new ResizeObserver(resize);
		resizeObserver.observe(canvas);
		window.addEventListener('pointermove', handlePointer, { passive: true });
		window.addEventListener('pointerleave', deactivatePointer, { passive: true });
		motionQuery.addEventListener('change', handleMotionChange);
		document.addEventListener('visibilitychange', handleVisibilityChange);

		const intersection = new IntersectionObserver(
			(entries) => {
				visible = entries[0]?.isIntersecting ?? true;
				if (visible) startLoop();
				else stopLoop();
			},
			{ threshold: 0 }
		);
		intersection.observe(canvas);

		return () => {
			resizeObserver.disconnect();
			intersection.disconnect();
			window.removeEventListener('pointermove', handlePointer);
			window.removeEventListener('pointerleave', deactivatePointer);
			motionQuery.removeEventListener('change', handleMotionChange);
			document.removeEventListener('visibilitychange', handleVisibilityChange);
			stopLoop();
		};
	});
</script>

<canvas bind:this={canvas} aria-hidden="true" class="pointer-events-none absolute inset-0 h-full w-full"></canvas>
