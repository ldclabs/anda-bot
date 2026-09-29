<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import type { EntityClaim, MemoryEntity } from './api'
  import { GRAPH_WIDTH, layoutEntityGraph } from './entity'
  import { actorLabel, entityName } from './labels'

  let {
    entity,
    claims,
    onopen
  }: {
    entity: MemoryEntity
    claims: EntityClaim[]
    onopen: (id: string, label: string) => void
  } = $props()

  let width = $state(0)
  const name = $derived(entityName(entity))
  // Rows stay about 34px apart at any width, so labels never touch; narrow
  // screens get fewer of them.
  const row = $derived(width ? Math.max(8, Math.ceil((34 * GRAPH_WIDTH) / width)) : 8)
  const layout = $derived(layoutEntityGraph(entity.id, claims, width && width < 480 ? 6 : 12, row))
  // Four theme hues stay distinct in both themes; further relations share a
  // neutral colour and are told apart by the legend.
  const color = (group: number) =>
    group < 4 ? `var(--chart-${group + 1})` : 'var(--muted-foreground)'
  const left = (x: number) => `${(x / GRAPH_WIDTH) * 100}%`
  const top = (y: number) => `${(y / layout.height) * 100}%`
</script>

{#if layout.nodes.length}
  <figure class="my-7" aria-label={getMessage('memoryEntityGraph')}>
    <div
      class="relative w-full overflow-hidden rounded-md border border-border bg-muted/20 {layout
        .nodes.length <= 4
        ? 'max-w-lg'
        : ''}"
      style="aspect-ratio: {GRAPH_WIDTH} / {layout.height}"
      bind:clientWidth={width}
    >
      <svg
        viewBox="0 0 {GRAPH_WIDTH} {layout.height}"
        class="absolute inset-0 size-full"
        aria-hidden="true"
      >
        {#each layout.edges as edge (edge.id)}
          <line
            x1={edge.x1}
            y1={edge.y1}
            x2={edge.x2}
            y2={edge.y2}
            stroke={color(edge.group)}
            stroke-width="1.5"
            stroke-dasharray={edge.current ? undefined : '4 4'}
            stroke-opacity={edge.current ? 0.85 : 0.45}
            vector-effect="non-scaling-stroke"
          />
          <polygon
            points={edge.arrow}
            fill={color(edge.group)}
            fill-opacity={edge.current ? 0.85 : 0.45}
          />
        {/each}
      </svg>
      <p
        class="absolute max-w-[22%] -translate-x-1/2 -translate-y-1/2 truncate rounded-full bg-foreground px-3 py-1 text-xs font-medium text-background"
        style="left: 50%; top: 50%"
        title={name}
      >
        {name}
      </p>
      {#each layout.nodes as node (node.id)}
        <button
          class="absolute max-w-[26%] -translate-x-1/2 -translate-y-1/2 cursor-pointer truncate rounded-full border-2 bg-background px-2 py-0.5 text-xs shadow-xs hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring {node.current
            ? ''
            : 'border-dashed text-muted-foreground'}"
          style="left: {left(node.x)}; top: {top(node.y)}; border-color: {color(node.group)}"
          title={actorLabel(node.label)}
          aria-label={getMessage('memoryOpenEntity', actorLabel(node.label))}
          onclick={() => onopen(node.id, actorLabel(node.label))}>{actorLabel(node.label)}</button
        >
      {/each}
    </div>
    <figcaption class="mt-3 text-xs leading-relaxed text-muted-foreground">
      <ul class="flex flex-wrap gap-x-4 gap-y-1">
        {#each layout.relations as relation (relation.group)}
          <li class="flex items-center gap-1.5">
            <span
              class="inline-block h-0.5 w-4 rounded-full"
              style="background: {color(relation.group)}"
            ></span>
            {relation.direction === 'outgoing'
              ? `${name} · ${relation.predicate} →`
              : `→ ${relation.predicate} · ${name}`} ({relation.nodes})
          </li>
        {/each}
      </ul>
      <p class="mt-2">
        {getMessage('memoryEntityGraphHint')}{#if layout.hidden}
          {' '}{getMessage('memoryEntityGraphMore', String(layout.hidden))}{/if}
      </p>
    </figcaption>
  </figure>
{/if}
