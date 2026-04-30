<script lang="ts">
	import type { HTMLAttributes } from "svelte/elements";
	import { cn, type WithElementRef } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		size = "default",
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> & { size?: "default" | "sm" } = $props();
</script>

<div
	bind:this={ref}
	data-slot="card"
	data-size={size}
	class={cn("text-[color:var(--anda-parchment)] gap-6 overflow-hidden rounded-lg border border-white/10 bg-white/[0.04] backdrop-blur-xl py-6 text-sm transition-colors hover:border-white/20 hover:bg-white/[0.06] has-[>img:first-child]:pt-0 data-[size=sm]:gap-4 data-[size=sm]:py-4 *:[img:first-child]:rounded-t-lg *:[img:last-child]:rounded-b-lg group/card flex flex-col", className)}
	{...restProps}
>
	{@render children?.()}
</div>
