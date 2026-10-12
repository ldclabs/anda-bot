<script lang="ts">
  /**
   * Where a workbench panel sits: along an edge of the canvas, filling it or
   * tiled with the rest, or in a window of its own. A panel in its own window
   * goes back into the canvas from here. Panels also move by dragging their
   * title onto another pane.
   */
  import { DropdownMenu } from 'bits-ui'
  import {
    AppWindow,
    Columns2,
    PanelBottom,
    PanelLeft,
    PanelRight,
    PanelTop,
    PictureInPicture2,
    Square
  } from '@lucide/svelte'
  import { isPanelEdge, type PanelEdge } from '../shared/panels'
  import type { Label } from './labels'
  import { tip } from './tooltip'

  let {
    t,
    edge,
    rtl,
    detached = false,
    filled = false,
    onMove,
    onReturn,
    onFill,
    onDetach
  }: {
    t: (key: Label) => string
    /** The canvas edge the panel runs along, if any. */
    edge: PanelEdge | null
    /** Right to left: the canvas starts on the right. */
    rtl: boolean
    detached?: boolean
    filled?: boolean
    /** Along that edge of the canvas; from its own window, back in the main window. */
    onMove: (edge: PanelEdge) => void
    /** From its own window: back into the canvas where panels open. */
    onReturn?: () => void
    onFill?: (filled: boolean) => void
    onDetach?: () => void
  } = $props()

  // Edges are in reading order; icons and names say which side of the screen.
  const left = { label: 'dockLeft' as const, icon: PanelLeft }
  const right = { label: 'dockRight' as const, icon: PanelRight }
  const edges: { id: PanelEdge; label: Label; icon: typeof PanelLeft }[] = $derived([
    { id: 'start', ...(rtl ? right : left) },
    { id: 'end', ...(rtl ? left : right) },
    { id: 'top', label: 'dockTop', icon: PanelTop },
    { id: 'bottom', label: 'dockBottom', icon: PanelBottom }
  ])
  const Trigger = $derived(
    detached ? AppWindow : (edges.find((item) => item.id === edge)?.icon ?? Columns2)
  )
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <button
        {...props}
        class="icon-button"
        aria-label={t('panelLayout')}
        use:tip={t('panelLayout')}><Trigger size={15} /></button
      >
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      class="panel-layout-menu"
      data-slot="dropdown-menu-content"
      align="end"
      sideOffset={6}
    >
      <DropdownMenu.Group>
        <DropdownMenu.GroupHeading class="panel-layout-heading"
          >{t(detached ? 'dockInMain' : 'movePanel')}</DropdownMenu.GroupHeading
        >
        <DropdownMenu.RadioGroup
          class="panel-layout-choices"
          value={detached ? '' : (edge ?? '')}
          onValueChange={(value) => {
            if (isPanelEdge(value)) onMove(value)
          }}
        >
          {#each edges as item (item.id)}
            <DropdownMenu.RadioItem value={item.id}>
              {#snippet child({ props })}
                <div {...props} aria-label={t(item.label)} use:tip={t(item.label)}>
                  <item.icon size={16} />
                </div>
              {/snippet}
            </DropdownMenu.RadioItem>
          {/each}
        </DropdownMenu.RadioGroup>
      </DropdownMenu.Group>
      {#if !detached && onFill}
        <DropdownMenu.Separator />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="panel-layout-heading"
            >{t('fillArrange')}</DropdownMenu.GroupHeading
          >
          <DropdownMenu.RadioGroup
            class="panel-layout-choices"
            value={filled ? 'fill' : 'split'}
            onValueChange={(value) => onFill(value === 'fill')}
          >
            <DropdownMenu.RadioItem value="fill">
              {#snippet child({ props })}
                <div {...props} aria-label={t('fillPanel')} use:tip={t('fillPanel')}>
                  <Square size={16} />
                </div>
              {/snippet}
            </DropdownMenu.RadioItem>
            <DropdownMenu.RadioItem value="split">
              {#snippet child({ props })}
                <div {...props} aria-label={t('splitPanel')} use:tip={t('splitPanel')}>
                  <Columns2 size={16} />
                </div>
              {/snippet}
            </DropdownMenu.RadioItem>
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Group>
      {/if}
      <DropdownMenu.Separator />
      {#if detached && onReturn}
        <DropdownMenu.Item class="panel-layout-command" onSelect={onReturn}
          ><PictureInPicture2 size={15} />{t('returnToMain')}</DropdownMenu.Item
        >
      {:else if onDetach}
        <DropdownMenu.Item class="panel-layout-command" onSelect={onDetach}
          ><AppWindow size={15} />{t('openInWindow')}</DropdownMenu.Item
        >
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
