import { cn } from '$lib/utils'

type ClassName = string | false | null | undefined

export type ButtonVariant = 'default' | 'outline' | 'secondary' | 'ghost' | 'destructive' | 'link'
export type ButtonSize = 'default' | 'xs' | 'sm' | 'lg' | 'icon' | 'icon-xs' | 'icon-sm' | 'icon-lg'
export type BadgeVariant = 'default' | 'secondary' | 'destructive' | 'outline' | 'ghost' | 'link'
export type ItemVariant = 'default' | 'outline' | 'muted'
export type ItemSize = 'default' | 'sm' | 'xs'
export type ItemMediaVariant = 'default' | 'icon' | 'image'

const buttonBase =
  "focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 rounded-md border border-transparent bg-clip-padding text-sm font-medium focus-visible:ring-3 active:not-aria-[haspopup]:translate-y-px aria-invalid:ring-3 [&_svg:not([class*='size-'])]:size-4 group/button inline-flex shrink-0 items-center justify-center whitespace-nowrap transition-all outline-none select-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0"

const buttonVariants: Record<ButtonVariant, string> = {
  default: 'bg-primary text-primary-foreground hover:bg-primary/80',
  outline:
    'border-border bg-background hover:bg-muted hover:text-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/50 aria-expanded:bg-muted aria-expanded:text-foreground shadow-xs',
  secondary:
    'bg-secondary text-secondary-foreground hover:bg-secondary/80 aria-expanded:bg-secondary aria-expanded:text-secondary-foreground',
  ghost:
    'hover:bg-muted hover:text-foreground dark:hover:bg-muted/50 aria-expanded:bg-muted aria-expanded:text-foreground',
  destructive:
    'bg-destructive/10 hover:bg-destructive/20 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 dark:bg-destructive/20 text-destructive focus-visible:border-destructive/40 dark:hover:bg-destructive/30',
  link: 'text-primary underline-offset-4 hover:underline'
}

const buttonSizes: Record<ButtonSize, string> = {
  default:
    'h-9 gap-1.5 px-2.5 in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2',
  xs: "h-6 gap-1 rounded-[min(var(--radius-md),8px)] px-2 text-xs in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
  sm: 'h-8 gap-1 rounded-[min(var(--radius-md),10px)] px-2.5 in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5',
  lg: 'h-10 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2',
  icon: 'size-9',
  'icon-xs':
    "size-6 rounded-[min(var(--radius-md),8px)] in-data-[slot=button-group]:rounded-md [&_svg:not([class*='size-'])]:size-3",
  'icon-sm': 'size-8 rounded-[min(var(--radius-md),10px)] in-data-[slot=button-group]:rounded-md',
  'icon-lg': 'size-10'
}

const badgeBase =
  'h-5 gap-1 rounded-4xl border border-transparent px-2 py-0.5 text-xs font-medium has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&>svg]:size-3! focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive group/badge inline-flex w-fit shrink-0 items-center justify-center overflow-hidden whitespace-nowrap transition-colors focus-visible:ring-[3px] [&>svg]:pointer-events-none'

const badgeVariants: Record<BadgeVariant, string> = {
  default: 'bg-primary text-primary-foreground [a]:hover:bg-primary/80',
  secondary: 'bg-secondary text-secondary-foreground [a]:hover:bg-secondary/80',
  destructive:
    'bg-destructive/10 [a]:hover:bg-destructive/20 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 text-destructive dark:bg-destructive/20',
  outline: 'border-border text-foreground [a]:hover:bg-muted [a]:hover:text-muted-foreground',
  ghost: 'hover:bg-muted hover:text-muted-foreground dark:hover:bg-muted/50',
  link: 'text-primary underline-offset-4 hover:underline'
}

const itemBase =
  '[a]:hover:bg-muted rounded-md border text-sm group/item focus-visible:border-ring focus-visible:ring-ring/50 flex w-full flex-wrap items-center transition-colors duration-100 outline-none focus-visible:ring-[3px] [a]:transition-colors'

const itemVariants: Record<ItemVariant, string> = {
  default: 'border-transparent',
  outline: 'border-border',
  muted: 'bg-muted/50 border-transparent'
}

const itemSizes: Record<ItemSize, string> = {
  default: 'gap-3.5 px-4 py-3.5',
  sm: 'gap-2.5 px-3 py-2.5',
  xs: 'gap-2 px-2.5 py-2 in-data-[slot=dropdown-menu-content]:p-0'
}

const itemMediaBase =
  'gap-2 group-has-data-[slot=item-description]/item:translate-y-0.5 group-has-data-[slot=item-description]/item:self-start flex shrink-0 items-center justify-center [&_svg]:pointer-events-none'

const itemMediaVariants: Record<ItemMediaVariant, string> = {
  default: 'bg-transparent',
  icon: "[&_svg:not([class*='size-'])]:size-4",
  image:
    'size-10 overflow-hidden rounded-sm group-data-[size=sm]/item:size-8 group-data-[size=xs]/item:size-6 [&_img]:size-full [&_img]:object-cover'
}

export function buttonClass(
  variant: ButtonVariant = 'default',
  size: ButtonSize = 'default',
  className?: ClassName
) {
  return cn(buttonBase, buttonVariants[variant], buttonSizes[size], className)
}

export function badgeClass(variant: BadgeVariant = 'default', className?: ClassName) {
  return cn(badgeBase, badgeVariants[variant], className)
}

export function itemClass(
  variant: ItemVariant = 'default',
  size: ItemSize = 'default',
  className?: ClassName
) {
  return cn(itemBase, itemVariants[variant], itemSizes[size], className)
}

export function itemMediaClass(variant: ItemMediaVariant = 'default', className?: ClassName) {
  return cn(itemMediaBase, itemMediaVariants[variant], className)
}

export function itemContentClass(className?: ClassName) {
  return cn(
    'gap-1 group-data-[size=xs]/item:gap-0 flex flex-1 flex-col [&+[data-slot=item-content]]:flex-none',
    className
  )
}

export function itemTitleClass(className?: ClassName) {
  return cn(
    'gap-2 text-sm leading-snug font-medium underline-offset-4 line-clamp-1 flex w-fit items-center',
    className
  )
}

export function cardClass(className?: ClassName) {
  return cn(
    'group/card flex flex-col gap-6 overflow-hidden rounded-xl bg-card py-6 text-sm text-card-foreground shadow-xs ring-1 ring-foreground/10 has-[>img:first-child]:pt-0 data-[size=sm]:gap-4 data-[size=sm]:py-4 *:[img:first-child]:rounded-t-xl *:[img:last-child]:rounded-b-xl',
    className
  )
}

export function cardContentClass(className?: ClassName) {
  return cn('px-6 group-data-[size=sm]/card:px-4', className)
}

export function cardHeaderClass(className?: ClassName) {
  return cn(
    'group/card-header @container/card-header grid auto-rows-min items-start gap-1 rounded-t-xl px-6 group-data-[size=sm]/card:px-4 has-data-[slot=card-action]:grid-cols-[1fr_auto] has-data-[slot=card-description]:grid-rows-[auto_auto] [.border-b]:pb-6 group-data-[size=sm]/card:[.border-b]:pb-4',
    className
  )
}

export function cardTitleClass(className?: ClassName) {
  return cn('text-base leading-normal font-medium group-data-[size=sm]/card:text-sm', className)
}

export function inputClass(className?: ClassName) {
  return cn(
    'dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 h-9 rounded-md border bg-transparent px-2.5 py-1 text-base shadow-xs transition-[color,box-shadow] file:h-7 file:text-sm file:font-medium focus-visible:ring-3 aria-invalid:ring-3 md:text-sm file:text-foreground placeholder:text-muted-foreground w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50',
    className
  )
}

export function textareaClass(className?: ClassName) {
  return cn(
    'border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 rounded-md border bg-transparent px-2.5 py-2 text-base shadow-xs transition-[color,box-shadow] focus-visible:ring-3 aria-invalid:ring-3 md:text-sm placeholder:text-muted-foreground flex field-sizing-content min-h-16 w-full outline-none disabled:cursor-not-allowed disabled:opacity-50',
    className
  )
}

export function inputGroupClass(className?: ClassName) {
  return cn(
    'group/input-group border-input dark:bg-input/30 has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-ring/50 has-[[data-slot][aria-invalid=true]]:ring-destructive/20 has-[[data-slot][aria-invalid=true]]:border-destructive dark:has-[[data-slot][aria-invalid=true]]:ring-destructive/40 h-9 rounded-md border shadow-xs transition-[color,box-shadow] in-data-[slot=combobox-content]:focus-within:border-inherit in-data-[slot=combobox-content]:focus-within:ring-0 has-[[data-slot=input-group-control]:focus-visible]:ring-3 has-[[data-slot][aria-invalid=true]]:ring-3 has-[>[data-align=block-end]]:h-auto has-[>[data-align=block-end]]:flex-col has-[>[data-align=block-start]]:h-auto has-[>[data-align=block-start]]:flex-col has-[>[data-align=block-end]]:[&>input]:pt-3 has-[>[data-align=block-start]]:[&>input]:pb-3 has-[>[data-align=inline-end]]:[&>input]:pr-1.5 has-[>[data-align=inline-start]]:[&>input]:pl-1.5 relative flex w-full min-w-0 items-center outline-none has-[>textarea]:h-auto',
    className
  )
}

export function fieldGroupClass(className?: ClassName) {
  return cn(
    'gap-7 data-[slot=checkbox-group]:gap-3 *:data-[slot=field-group]:gap-4 group/field-group @container/field-group flex w-full flex-col',
    className
  )
}

export function fieldClass(className?: ClassName) {
  return cn(
    'data-[invalid=true]:text-destructive gap-3 group/field flex w-full flex-col',
    className
  )
}

export function fieldLabelClass(className?: ClassName) {
  return cn(
    'has-data-checked:bg-primary/5 has-data-checked:border-primary/30 dark:has-data-checked:border-primary/20 dark:has-data-checked:bg-primary/10 gap-2 leading-snug group-data-[disabled=true]/field:opacity-50 has-[>[data-slot=field]]:rounded-md has-[>[data-slot=field]]:border *:data-[slot=field]:p-3 group/field-label peer/field-label flex w-fit has-[>[data-slot=field]]:w-full has-[>[data-slot=field]]:flex-col',
    className
  )
}

export function nativeSelectWrapperClass(className?: ClassName) {
  return cn(
    'cn-native-select-wrapper group/native-select relative w-fit has-[select:disabled]:opacity-50',
    className
  )
}

export function nativeSelectClass(className?: ClassName) {
  return cn(
    'border-input placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground dark:bg-input/30 dark:hover:bg-input/50 focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 h-9 w-full min-w-0 appearance-none rounded-md border bg-transparent py-1 pr-8 pl-2.5 text-sm shadow-xs transition-[color,box-shadow] select-none focus-visible:ring-3 aria-invalid:ring-3 data-[size=sm]:h-8 outline-none disabled:pointer-events-none disabled:cursor-not-allowed',
    className
  )
}

export function separatorClass(className?: ClassName) {
  return cn(
    'bg-border shrink-0 data-[orientation=horizontal]:h-px data-[orientation=horizontal]:w-full data-[orientation=vertical]:h-full data-[orientation=vertical]:w-px',
    className
  )
}

export function alertClass(className?: ClassName) {
  return cn(
    "grid gap-0.5 rounded-lg border px-4 py-3 text-left text-sm has-data-[slot=alert-action]:relative has-data-[slot=alert-action]:pr-18 has-[>svg]:grid-cols-[auto_1fr] has-[>svg]:gap-x-2.5 *:[svg]:row-span-2 *:[svg]:translate-y-0.5 *:[svg]:text-current *:[svg:not([class*='size-'])]:size-4 group/alert relative w-full bg-card text-card-foreground",
    className
  )
}

export function alertDescriptionClass(className?: ClassName) {
  return cn(
    'text-muted-foreground text-sm text-balance md:text-pretty [&_p:not(:last-child)]:mb-4 [&_a]:hover:text-foreground [&_a]:underline [&_a]:underline-offset-3',
    className
  )
}

export function dialogOverlayClass(className?: ClassName) {
  return cn(
    'data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 bg-black/10 duration-100 supports-backdrop-filter:backdrop-blur-xs fixed inset-0 isolate z-50',
    className
  )
}

export function dialogContentClass(className?: ClassName) {
  return cn(
    'bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 ring-foreground/10 grid max-w-[calc(100%-2rem)] gap-6 rounded-xl p-6 text-sm ring-1 duration-100 sm:max-w-md fixed top-1/2 left-1/2 z-50 w-full -translate-x-1/2 -translate-y-1/2 outline-none',
    className
  )
}

export function dialogDescriptionClass(className?: ClassName) {
  return cn(
    'text-muted-foreground *:[a]:hover:text-foreground text-sm *:[a]:underline *:[a]:underline-offset-3',
    className
  )
}

export function alertDialogOverlayClass(className?: ClassName) {
  return cn(
    'fixed inset-0 z-50 bg-black/10 duration-100 supports-backdrop-filter:backdrop-blur-xs data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0',
    className
  )
}

export function alertDialogContentClass(className?: ClassName) {
  return cn(
    'group/alert-dialog-content fixed top-1/2 left-1/2 z-50 grid w-full max-w-xs -translate-x-1/2 -translate-y-1/2 gap-6 rounded-xl bg-popover p-6 text-popover-foreground ring-1 ring-foreground/10 duration-100 outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95',
    className
  )
}

export function alertDialogDescriptionClass(className?: ClassName) {
  return cn(
    'text-sm text-balance text-muted-foreground md:text-pretty *:[a]:underline *:[a]:underline-offset-3 *:[a]:hover:text-foreground',
    className
  )
}

export function tooltipContentClass(className?: ClassName) {
  return cn(
    'data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=delayed-open]:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs has-data-[slot=kbd]:pr-1.5 **:data-[slot=kbd]:relative **:data-[slot=kbd]:isolate **:data-[slot=kbd]:z-50 **:data-[slot=kbd]:rounded-sm bg-foreground text-background z-50 w-fit max-w-xs origin-(--bits-tooltip-content-transform-origin)',
    className
  )
}

export function tooltipArrowClass(className?: ClassName) {
  return cn(
    'size-2.5 translate-y-[calc(-50%-2px)] rotate-45 rounded-[2px] bg-foreground fill-foreground z-50 data-[side=top]:translate-x-1/2 data-[side=top]:translate-y-[calc(-50%+2px)] data-[side=bottom]:-translate-x-1/2 data-[side=bottom]:-translate-y-[calc(-50%+1px)] data-[side=right]:translate-x-[calc(50%+2px)] data-[side=right]:translate-y-1/2 data-[side=left]:-translate-y-[calc(50%-3px)]',
    className
  )
}
