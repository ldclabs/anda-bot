<script lang="ts">
	import { onMount } from 'svelte';

	type NexusNode = {
		x: number;
		y: number;
		vx: number;
		vy: number;
		radius: number;
		depth: number;
		tone: number;
		label: string;
	};

	let canvas: HTMLCanvasElement;

	const labels = [
		'identity',
		'projects',
		'preferences',
		'events',
		'timelines',
		'tools',
		'channels',
		'skills',
		'recall',
		'formation',
		'sleep',
		'actions'
	];

	onMount(() => {
		const maybeContext = canvas.getContext('2d');
		if (!maybeContext) return;
		const context: CanvasRenderingContext2D = maybeContext;

		let width = 0;
		let height = 0;
		let frame = 0;
		let animation = 0;
		let nodes: NexusNode[] = [];
		const pointer = { x: 0, y: 0, active: false };
		const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		const palette = ['#f1a64e', '#35b2ab', '#d84b42', '#f4eee3', '#7dd3c7'];

		function seed(index: number) {
			const value = Math.sin(index * 12.9898 + 78.233) * 43758.5453;
			return value - Math.floor(value);
		}

		function resize() {
			const rect = canvas.getBoundingClientRect();
			const dpr = Math.min(window.devicePixelRatio || 1, 2);
			width = Math.max(rect.width, 320);
			height = Math.max(rect.height, 420);
			canvas.width = Math.floor(width * dpr);
			canvas.height = Math.floor(height * dpr);
			context.setTransform(dpr, 0, 0, dpr, 0, 0);

			const total = width < 720 ? 42 : 68;
			nodes = Array.from({ length: total }, (_, index) => {
				const angle = seed(index) * Math.PI * 2;
				const orbit = 0.14 + seed(index + 11) * 0.46;
				const centerBias = index % 7 === 0 ? 0.32 : 1;
				return {
					x: width * (0.52 + Math.cos(angle) * orbit * centerBias),
					y: height * (0.48 + Math.sin(angle) * orbit * 0.78 * centerBias),
					vx: (seed(index + 21) - 0.5) * 0.18,
					vy: (seed(index + 31) - 0.5) * 0.18,
					radius: 1.6 + seed(index + 41) * 3.4,
					depth: 0.35 + seed(index + 51) * 0.85,
					tone: index % palette.length,
					label: labels[index % labels.length]
				};
			});
		}

		function draw() {
			frame += reducedMotion ? 0.28 : 1;
			context.clearRect(0, 0, width, height);

			const base = context.createLinearGradient(0, 0, width, height);
			base.addColorStop(0, '#111617');
			base.addColorStop(0.42, '#18251f');
			base.addColorStop(0.68, '#271915');
			base.addColorStop(1, '#081412');
			context.fillStyle = base;
			context.fillRect(0, 0, width, height);

			const halo = context.createRadialGradient(
				width * 0.64,
				height * 0.38,
				0,
				width * 0.64,
				height * 0.38,
				Math.max(width, height) * 0.56
			);
			halo.addColorStop(0, 'rgba(241, 166, 78, 0.22)');
			halo.addColorStop(0.36, 'rgba(53, 178, 171, 0.14)');
			halo.addColorStop(1, 'rgba(8, 20, 18, 0)');
			context.fillStyle = halo;
			context.fillRect(0, 0, width, height);

			for (const node of nodes) {
				node.x += node.vx * node.depth;
				node.y += node.vy * node.depth;
				const pullX = width * 0.58 - node.x;
				const pullY = height * 0.48 - node.y;
				node.vx += pullX * 0.000006;
				node.vy += pullY * 0.000006;

				if (pointer.active) {
					const dx = node.x - pointer.x;
					const dy = node.y - pointer.y;
					const distance = Math.hypot(dx, dy) || 1;
					if (distance < 220) {
						node.vx += (dx / distance) * 0.012;
						node.vy += (dy / distance) * 0.012;
					}
				}

				node.vx *= 0.996;
				node.vy *= 0.996;
				if (node.x < -40) node.x = width + 40;
				if (node.x > width + 40) node.x = -40;
				if (node.y < -40) node.y = height + 40;
				if (node.y > height + 40) node.y = -40;
			}

			for (let first = 0; first < nodes.length; first += 1) {
				for (let second = first + 1; second < nodes.length; second += 1) {
					const a = nodes[first];
					const b = nodes[second];
					const dx = a.x - b.x;
					const dy = a.y - b.y;
					const distance = Math.hypot(dx, dy);
					const threshold = 132 * Math.min(a.depth + b.depth, 1.5);
					if (distance < threshold) {
						const alpha = (1 - distance / threshold) * 0.26;
						context.strokeStyle = `rgba(244,238,227,${alpha})`;
						context.lineWidth = 0.8;
						context.beginPath();
						context.moveTo(a.x, a.y);
						context.lineTo(b.x, b.y);
						context.stroke();
					}
				}
			}

			nodes.forEach((node, index) => {
				const pulse = 1 + Math.sin(frame * 0.024 + index) * 0.18;
				const color = palette[node.tone];
				context.shadowColor = color;
				context.shadowBlur = 18 * node.depth;
				context.fillStyle = color;
				context.beginPath();
				context.arc(node.x, node.y, node.radius * pulse, 0, Math.PI * 2);
				context.fill();
				context.shadowBlur = 0;

				if (index % 9 === 0 && width > 720) {
					context.font = '12px Inter, sans-serif';
					context.fillStyle = 'rgba(244, 238, 227, 0.58)';
					context.fillText(node.label, node.x + 10, node.y - 8);
				}
			});

			context.fillStyle = 'rgba(244, 238, 227, 0.055)';
			for (let y = 0; y < height; y += 4) {
				context.fillRect(0, y, width, 1);
			}

			if (!reducedMotion) animation = requestAnimationFrame(draw);
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

		resize();
		draw();

		window.addEventListener('resize', resize);
		window.addEventListener('pointermove', handlePointer);
		window.addEventListener('pointerleave', deactivatePointer);

		return () => {
			window.removeEventListener('resize', resize);
			window.removeEventListener('pointermove', handlePointer);
			window.removeEventListener('pointerleave', deactivatePointer);
			cancelAnimationFrame(animation);
		};
	});
</script>

<canvas bind:this={canvas} aria-hidden="true" class="absolute inset-0 h-full w-full"></canvas>
