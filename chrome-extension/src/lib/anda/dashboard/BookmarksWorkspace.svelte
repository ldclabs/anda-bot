<script lang="ts">
  import { storeClientState } from '$lib/anda/client/platform'
  import { useAndaClient } from '$lib/anda/client/context'
  const andaClient = useAndaClient()
  import type { BookmarkedMessage } from '$lib/anda/client/types'
  import {
    BookmarkBrowser,
    bookmarkFolderIds,
    bookmarkPreviewText,
    type ActiveFolder
  } from '$lib/anda/bookmarks/browser.svelte'
  import { bookmarkJumpRequestStorageKey, createBookmarkJumpRequest } from '$lib/anda/bookmark-jump'
  import { buttonClass, inputClass } from '$lib/anda/ui'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { openAndaSidePanel } from '$lib/anda/dashboard/side-panel'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { formatTimestamp } from '$lib/utils/format'
  import { renderMarkdown } from '$lib/utils/markdown'
  import {
    Bookmark as BookmarkIcon,
    Check,
    ChevronDown,
    Copy,
    ExternalLink,
    FileText,
    Folder,
    LoaderCircle,
    Plus,
    RefreshCw,
    Search,
    Trash2,
    X
  } from '@lucide/svelte'
  import { onMount, untrack } from 'svelte'

  const browser = new BookmarkBrowser(andaClient.bookmarks)

  let newFolderName = $state('')
  let searchQuery = $state('')
  let selectedMessageId = $state('')
  let copiedMessageId = $state('')
  let copyError = $state('')
  /** The selected message's markdown; '' when its conversation lost it. */
  let detail = $state.raw<Promise<string> | null>(null)

  // A bookmarked message no longer changes, so each one's markdown is fetched
  // once per page view. Refresh drops the cache, and a failed fetch drops its
  // own entry so selecting the bookmark again retries.
  const markdownCache = new Map<string, Promise<string>>()

  const visibleItems = $derived.by(() => {
    const query = searchQuery.trim().toLowerCase()
    if (!query) {
      return browser.items
    }
    return browser.items.filter((item) => {
      return (
        bookmarkPreviewText(item).toLowerCase().includes(query) ||
        (item.source || '').toLowerCase().includes(query) ||
        bookmarkFolderIds(item).some((folderId) =>
          browser.folderName(folderId).toLowerCase().includes(query)
        )
      )
    })
  })

  const selectedItem = $derived<BookmarkedMessage | null>(
    visibleItems.find((item) => item.message_id === selectedMessageId) || visibleItems[0] || null
  )
  const selectedId = $derived(selectedItem?.message_id || '')
  /** The active folder's loaded count, with `+` while more pages remain. */
  const loadedCount = $derived(
    browser.loading ? '' : `${browser.items.length}${browser.hasMore ? '+' : ''}`
  )

  // Keyed by id alone: a search keystroke or folder change that rebuilds the
  // selected item must not refetch its message.
  $effect(() => {
    const id = selectedId
    copyError = ''
    detail = id ? untrack(() => loadMarkdown(selectedItem!)) : null
  })

  onMount(() => {
    andaClient
      .init({ conversations: false })
      .catch(() => undefined)
      .finally(() => {
        void browser.load()
      })
  })

  function loadMarkdown(item: BookmarkedMessage): Promise<string> {
    let markdown = markdownCache.get(item.message_id)
    if (!markdown) {
      markdown = andaClient.bookmarks.conversationMarkdown(item)
      markdownCache.set(item.message_id, markdown)
      markdown.catch(() => markdownCache.delete(item.message_id))
    }
    return markdown
  }

  function refresh() {
    markdownCache.clear()
    void browser.load()
  }

  function selectFolder(folder: ActiveFolder) {
    if (browser.activeFolder !== folder) {
      selectedMessageId = ''
    }
    void browser.selectFolder(folder)
  }

  async function createFolder() {
    if (await browser.createFolder(newFolderName)) {
      newFolderName = ''
    }
  }

  async function openSidePanel() {
    if (!selectedItem) {
      return
    }
    const { message_id, conversation, source } = selectedItem
    // The panel reads this on open and scrolls to the bookmarked message.
    void storeClientState({
      [bookmarkJumpRequestStorageKey]: createBookmarkJumpRequest({
        message_id,
        conversation,
        source
      })
    })
    await openAndaSidePanel()
  }

  async function copyMarkdown(messageId: string, markdown: string) {
    try {
      await navigator.clipboard.writeText(markdown)
    } catch (error) {
      copyError = errorToMessage(error)
      return
    }
    copyError = ''
    copiedMessageId = messageId
    window.setTimeout(() => {
      if (copiedMessageId === messageId) {
        copiedMessageId = ''
      }
    }, 1200)
  }

  function navItemClass(active: boolean): string {
    return active
      ? 'bg-background text-foreground shadow-xs'
      : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
  }
</script>

{#snippet pseudoFolder(folder: ActiveFolder, label: string)}
  {@const active = browser.activeFolder === folder}
  <button
    type="button"
    class={`flex min-w-0 items-center justify-between gap-2 rounded-md px-2.5 py-2 text-left text-sm transition ${navItemClass(active)}`}
    aria-current={active ? 'true' : undefined}
    onclick={() => selectFolder(folder)}
  >
    <span class="flex min-w-0 items-center gap-2">
      {#if folder === 'all'}
        <BookmarkIcon class="size-3.5 shrink-0" />
      {:else}
        <Folder class="size-3.5 shrink-0" />
      {/if}
      <span class="truncate">{label}</span>
    </span>
    {#if active}
      <span class="text-xs text-muted-foreground">{loadedCount}</span>
    {/if}
  </button>
{/snippet}

<!-- Three columns once the workspace is wide enough; below that the detail
     pane moves under the list so nothing scrolls sideways. -->
<div class="@container h-full min-h-0">
  <div
    class="grid h-full min-h-0 grid-cols-[9rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_minmax(0,45%)] bg-background @4xl:grid-cols-[10rem_minmax(0,2fr)_minmax(0,3fr)] @4xl:grid-rows-1"
  >
    <aside class="row-span-2 flex min-h-0 flex-col border-r bg-muted/20 @4xl:row-span-1">
      <div class="border-b px-3 py-3">
        <div class="flex items-center gap-2 text-sm font-bold">
          <BookmarkIcon class="size-4 text-emerald-800" />
          {getMessage('bookmarks')}
        </div>
        <p class="mt-1 text-xs leading-relaxed text-muted-foreground">
          {getMessage('bookmarksDashboardDescription')}
        </p>
      </div>

      <nav class="scrollbar-slim grid min-h-0 flex-1 content-start gap-1 overflow-y-auto p-2">
        {@render pseudoFolder('all', getMessage('allBookmarks'))}
        {@render pseudoFolder('unfiled', getMessage('unfiledBookmarks'))}

        {#each browser.folderList as { folder, depth, path } (folder._id)}
          {@const active = browser.activeFolder === folder._id}
          <div
            class={`group/folder grid grid-cols-[minmax(0,1fr)_auto] overflow-hidden rounded-md ${navItemClass(active)}`}
          >
            <button
              type="button"
              class="flex min-w-0 items-center justify-between gap-2 py-2 pe-1 text-left text-sm"
              style:padding-inline-start={`${0.625 + depth * 0.75}rem`}
              title={path}
              aria-current={active ? 'true' : undefined}
              onclick={() => selectFolder(folder._id)}
            >
              <span class="flex min-w-0 items-center gap-2">
                <Folder class="size-3.5 shrink-0" />
                <span class="truncate">{folder.name}</span>
              </span>
              {#if active}
                <span class="text-xs text-muted-foreground">{loadedCount}</span>
              {/if}
            </button>
            <button
              type="button"
              class="grid size-8 place-items-center text-muted-foreground hover:text-amber-700 disabled:opacity-50"
              disabled={browser.isDeletingFolder(folder._id)}
              aria-label={getMessage('deleteBookmarkFolder')}
              title={getMessage('deleteBookmarkFolder')}
              onclick={() => browser.deleteFolder(folder._id)}
            >
              {#if browser.isDeletingFolder(folder._id)}
                <LoaderCircle class="size-3 animate-spin" />
              {:else}
                <Trash2 class="size-3" />
              {/if}
            </button>
          </div>
        {/each}
      </nav>

      <form
        class="grid grid-cols-[minmax(0,1fr)_auto] gap-2 border-t p-2"
        onsubmit={(event) => (event.preventDefault(), createFolder())}
      >
        <input
          class={inputClass('h-8 text-xs')}
          bind:value={newFolderName}
          maxlength="80"
          placeholder={getMessage('bookmarkFolderNamePlaceholder')}
          aria-label={getMessage('bookmarkFolderNamePlaceholder')}
        />
        <button
          type="submit"
          class={buttonClass('outline', 'icon-sm')}
          disabled={browser.creatingFolder || !newFolderName.trim()}
          aria-label={getMessage('createBookmarkFolder')}
          title={getMessage('createBookmarkFolder')}
        >
          {#if browser.creatingFolder}
            <LoaderCircle class="size-4 animate-spin" />
          {:else}
            <Plus class="size-4" />
          {/if}
        </button>
      </form>
    </aside>

    <section class="flex min-h-0 min-w-0 flex-col">
      <div class="grid gap-3 border-b bg-background px-4 py-3">
        <div class="flex min-w-0 flex-wrap items-center justify-between gap-3">
          <div class="min-w-0">
            <h1 class="truncate text-base font-bold">{getMessage('bookmarksLibraryTitle')}</h1>
            <p class="truncate text-xs text-muted-foreground">
              {getMessage('bookmarksLibrarySubtitle')}
            </p>
          </div>
          <button
            type="button"
            class={buttonClass('outline', 'sm')}
            onclick={refresh}
            disabled={browser.loading}
          >
            <RefreshCw class={`size-3.5 ${browser.loading ? 'animate-spin' : ''}`} />
            {getMessage('refresh')}
          </button>
        </div>
        <label class="relative block min-w-0">
          <Search
            class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
          />
          <input
            class={inputClass('h-8 pl-8 text-sm')}
            bind:value={searchQuery}
            placeholder={getMessage('bookmarksSearchPlaceholder')}
            aria-label={getMessage('bookmarksSearchPlaceholder')}
          />
        </label>
      </div>

      <div class="scrollbar-slim min-h-0 flex-1 overflow-y-auto">
        {#if browser.loading}
          <div class="grid h-full min-h-60 place-items-center text-sm text-muted-foreground">
            <span class="flex items-center gap-2">
              <LoaderCircle class="size-4 animate-spin" />
              {getMessage('loading')}
            </span>
          </div>
        {:else if browser.error}
          <div
            class="grid h-full min-h-60 place-items-center px-6 text-center text-sm text-amber-700"
          >
            {browser.error}
          </div>
        {:else}
          {#if visibleItems.length === 0}
            <div
              class={`grid place-items-center px-6 text-center text-sm text-muted-foreground ${
                browser.hasMore ? 'min-h-40' : 'h-full min-h-60'
              }`}
            >
              <div class="grid max-w-72 gap-2">
                <BookmarkIcon class="mx-auto size-7" />
                <div class="font-medium">
                  {getMessage(searchQuery.trim() ? 'bookmarksNoMatches' : 'bookmarksEmpty')}
                </div>
              </div>
            </div>
          {:else}
            <div class="divide-y">
              {#each visibleItems as bookmark (bookmark.message_id)}
                {@const folderIds = bookmarkFolderIds(bookmark)}
                {@const createdAt = formatTimestamp(bookmark.created_at)}
                {@const available = browser.folderList.filter(
                  ({ folder }) => !folderIds.includes(folder._id)
                )}
                {@const selected = selectedId === bookmark.message_id}
                <article
                  class={`group grid gap-2 px-4 py-3 transition hover:bg-muted/35 ${
                    selected ? 'bg-muted/45' : ''
                  }`}
                >
                  <div class="flex min-w-0 items-start justify-between gap-3">
                    <button
                      type="button"
                      class="min-w-0 cursor-default text-left"
                      aria-current={selected ? 'true' : undefined}
                      onclick={() => (selectedMessageId = bookmark.message_id)}
                    >
                      <p class="line-clamp-2 text-sm leading-relaxed wrap-break-word">
                        {bookmarkPreviewText(bookmark)}
                      </p>
                      <div
                        class="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-muted-foreground"
                      >
                        {#if bookmark.source}
                          <span class="max-w-64 truncate" title={bookmark.source}
                            >{bookmark.source}</span
                          >
                        {/if}
                        {#if createdAt}
                          <span>{createdAt}</span>
                        {/if}
                      </div>
                    </button>
                    <button
                      type="button"
                      class={buttonClass(
                        'ghost',
                        'icon-sm',
                        'shrink-0 text-muted-foreground hover:text-amber-700'
                      )}
                      disabled={browser.isRemoving(bookmark.message_id)}
                      aria-label={getMessage('removeBookmark')}
                      title={getMessage('removeBookmark')}
                      onclick={() => browser.remove(bookmark.message_id)}
                    >
                      {#if browser.isRemoving(bookmark.message_id)}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <Trash2 class="size-4" />
                      {/if}
                    </button>
                  </div>

                  <div class="flex min-w-0 flex-wrap items-center gap-1">
                    {#each folderIds as folderId (folderId)}
                      {@const name = browser.folderName(folderId)}
                      {#if name}
                        <span
                          class="inline-flex h-6 max-w-40 items-center gap-1 rounded-md border bg-background px-2 text-[11px] text-muted-foreground"
                        >
                          <Folder class="size-3 shrink-0" />
                          <span class="truncate">{name}</span>
                          <button
                            type="button"
                            class="ml-0.5 rounded-sm text-muted-foreground transition hover:text-amber-700 disabled:opacity-50"
                            disabled={browser.isAssigning(bookmark.message_id)}
                            aria-label={getMessage('removeFromBookmarkFolder')}
                            title={getMessage('removeFromBookmarkFolder')}
                            onclick={() => browser.removeFromFolder(bookmark.message_id, folderId)}
                          >
                            <X class="size-3" />
                          </button>
                        </span>
                      {/if}
                    {/each}
                    {#if available.length > 0}
                      <DropdownMenu
                        class="h-6 max-w-44 gap-1 border bg-background px-2 text-[11px] text-muted-foreground focus-visible:border-ring"
                        items={available.map(({ folder, path }) => ({
                          value: String(folder._id),
                          label: path
                        }))}
                        disabled={browser.isAssigning(bookmark.message_id)}
                        onSelect={(folderId) =>
                          void browser.addToFolder(bookmark.message_id, Number(folderId))}
                      >
                        {#snippet trigger()}
                          <span class="truncate">{getMessage('addToBookmarkFolder')}</span>
                          <ChevronDown
                            class="size-3 shrink-0 opacity-60 group-data-[state=open]:rotate-180"
                            aria-hidden="true"
                          />
                        {/snippet}
                      </DropdownMenu>
                    {/if}
                  </div>
                </article>
              {/each}
            </div>
          {/if}

          {#if browser.hasMore}
            <div class="flex justify-center border-t py-3">
              <button
                type="button"
                class={buttonClass('outline', 'sm')}
                disabled={browser.loadingMore}
                onclick={() => browser.loadMore()}
              >
                {#if browser.loadingMore}
                  <LoaderCircle class="size-3.5 animate-spin" />
                {/if}
                {getMessage('loadMore')}
              </button>
            </div>
          {/if}
        {/if}
      </div>
    </section>

    <aside class="flex min-h-0 min-w-0 flex-col border-t bg-muted/20 @4xl:border-t-0 @4xl:border-l">
      <div class="flex items-start justify-between gap-3 border-b px-4 py-3">
        <div class="min-w-0">
          <div class="flex items-center gap-2 text-sm font-bold">
            <FileText class="size-4 text-emerald-800" />
            {getMessage('bookmarkContextTitle')}
          </div>
          <p class="mt-1 text-xs text-muted-foreground">{getMessage('bookmarkContextSubtitle')}</p>
        </div>
        {#if detail}
          {#await detail}
            <button
              type="button"
              class={buttonClass('outline', 'icon-sm', 'shrink-0')}
              disabled
              aria-label={getMessage('bookmarkCopyMarkdown')}
              title={getMessage('bookmarkCopyMarkdown')}
            >
              <LoaderCircle class="size-4 animate-spin" />
            </button>
          {:then markdown}
            {@const copied = copiedMessageId === selectedId}
            <button
              type="button"
              class={buttonClass('outline', 'icon-sm', 'shrink-0')}
              disabled={!markdown}
              aria-label={getMessage(copied ? 'bookmarkCopiedMarkdown' : 'bookmarkCopyMarkdown')}
              title={getMessage(copied ? 'bookmarkCopiedMarkdown' : 'bookmarkCopyMarkdown')}
              onclick={() => copyMarkdown(selectedId, markdown)}
            >
              {#if copied}
                <Check class="size-4" />
              {:else}
                <Copy class="size-4" />
              {/if}
            </button>
          {:catch}
            <button
              type="button"
              class={buttonClass('outline', 'icon-sm', 'shrink-0')}
              disabled
              aria-label={getMessage('bookmarkCopyMarkdown')}
              title={getMessage('bookmarkCopyMarkdown')}
            >
              <Copy class="size-4" />
            </button>
          {/await}
        {/if}
      </div>

      <div class="scrollbar-slim grid min-h-0 flex-1 content-start gap-4 overflow-y-auto p-4">
        {#if selectedItem && detail}
          {@const folderIds = bookmarkFolderIds(selectedItem)}
          <section class="grid gap-2 border-b pb-4">
            <div class="text-xs font-medium text-muted-foreground">
              {getMessage('bookmarkSelected')}
            </div>
            {#await detail}
              <div class="flex items-center gap-2 text-sm text-muted-foreground">
                <LoaderCircle class="size-4 animate-spin" />
                {getMessage('loading')}
              </div>
            {:then markdown}
              {#if markdown}
                <div class="md-content w-full min-w-0 text-pretty wrap-break-word">
                  {@html renderMarkdown(markdown)}
                </div>
              {:else}
                <p class="text-sm leading-relaxed wrap-break-word text-amber-700">
                  {getMessage('bookmarkNotLocated')}
                </p>
              {/if}
            {:catch error}
              <p class="text-sm leading-relaxed wrap-break-word text-amber-700">
                {errorToMessage(error)}
              </p>
            {/await}
            {#if copyError}
              <p class="text-xs wrap-break-word text-amber-700">{copyError}</p>
            {/if}
            <div class="grid gap-1 text-xs text-muted-foreground">
              {#if selectedItem.source}
                <span class="truncate">{selectedItem.source}</span>
              {/if}
              <span>{formatTimestamp(selectedItem.created_at) || selectedItem.message_id}</span>
            </div>
            <button
              type="button"
              class={buttonClass('outline', 'sm', 'mt-1 justify-between')}
              onclick={openSidePanel}
            >
              <span class="truncate">{getMessage('bookmarkOpenChat')}</span>
              <ExternalLink class="size-3.5" />
            </button>
          </section>

          <section class="grid gap-2">
            <div class="flex items-center gap-2 text-xs font-semibold">
              <Folder class="size-3.5" />
              {getMessage('bookmarkFolders')}
            </div>
            <div class="flex flex-wrap gap-1">
              {#each folderIds as folderId (folderId)}
                {@const name = browser.folderName(folderId)}
                {#if name}
                  <span
                    class="rounded-md border bg-background px-2 py-1 text-[11px] text-muted-foreground"
                  >
                    {name}
                  </span>
                {/if}
              {/each}
              {#if folderIds.length === 0}
                <span class="text-xs text-muted-foreground">{getMessage('unfiledBookmarks')}</span>
              {/if}
            </div>
          </section>
        {:else}
          <div class="grid min-h-40 place-items-center text-center text-sm text-muted-foreground">
            {getMessage(searchQuery.trim() ? 'bookmarksNoMatches' : 'bookmarksEmpty')}
          </div>
        {/if}
      </div>
    </aside>
  </div>
</div>
