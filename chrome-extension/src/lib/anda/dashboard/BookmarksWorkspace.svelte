<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import type {
    Bookmark,
    BookmarkFolder,
    BookmarkFolders,
    BookmarkedMessage
  } from '$lib/anda/client/types'
  import { bookmarkJumpRequestStorageKey, createBookmarkJumpRequest } from '$lib/anda/bookmark-jump'
  import { buttonClass, inputClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { renderMarkdown } from '$lib/utils/markdown'
  import {
    Bookmark as BookmarkIcon,
    BrainCircuit,
    Check,
    Copy,
    ExternalLink,
    Folder,
    LoaderCircle,
    Plus,
    RefreshCw,
    Search,
    Trash2,
    X
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  type ActiveFolder = 'all' | 'unfiled' | number

  const emptyFolders = (): BookmarkFolders => ({
    version: 1,
    next_folder_id: 1,
    folders: {},
    updated_at: 0
  })

  let items = $state<BookmarkedMessage[]>([])
  let folders = $state<BookmarkFolders>(emptyFolders())
  let activeFolder = $state<ActiveFolder>('all')
  let newFolderName = $state('')
  let searchQuery = $state('')
  let selectedMessageId = $state('')
  let cursor = $state<string | null>(null)
  let loading = $state(false)
  let loadingMore = $state(false)
  let creatingFolder = $state(false)
  let removingIds = $state(new Set<string>())
  let deletingFolderIds = $state(new Set<number>())
  let assigningIds = $state(new Set<string>())
  let copiedMessageId = $state('')
  let selectedDetailMessageId = $state('')
  let selectedDetailMarkdown = $state('')
  let selectedDetailLoading = $state(false)
  let selectedDetailError = $state('')
  let error = $state('')

  const visibleItems = $derived.by(() => {
    const query = searchQuery.trim().toLowerCase()
    if (!query) {
      return items
    }
    return items.filter((item) => {
      return (
        previewText(item).toLowerCase().includes(query) ||
        (item.source || '').toLowerCase().includes(query) ||
        bookmarkFolderIds(item).some((folderId) =>
          folderName(folderId).toLowerCase().includes(query)
        )
      )
    })
  })

  const selectedItem = $derived.by<BookmarkedMessage | null>(() => {
    return (
      visibleItems.find((item) => item.message_id === selectedMessageId) || visibleItems[0] || null
    )
  })

  const selectedDetailConceptTokens = $derived.by(() =>
    selectedDetailMarkdown
      .split(/\s+/)
      .map((token) => token.replace(/[^\p{L}\p{N}_-]/gu, ''))
      .filter(Boolean)
      .slice(0, 5)
  )
  const [selectedDetailHtml, selectedDetailHook] = $derived.by(() =>
    renderMarkdown(selectedDetailMarkdown)
  )

  $effect(() => {
    const bookmark = selectedItem
    selectedDetailMessageId = bookmark?.message_id || ''
    selectedDetailMarkdown = ''
    selectedDetailError = ''

    if (!bookmark) {
      selectedDetailLoading = false
      return
    }

    selectedDetailLoading = true
    void loadSelectedDetailMarkdown(bookmark, bookmark.message_id)
  })

  $effect(() => {
    selectedDetailMarkdown
    void selectedDetailHook()
  })

  onMount(() => {
    andaClient
      .init()
      .catch(() => undefined)
      .finally(() => {
        void loadFirstPage()
      })
  })

  async function loadFirstPage() {
    loading = true
    error = ''
    try {
      folders = await andaClient.listBookmarkFolders()
      const { items: page, nextCursor } = await listActiveBookmarks()
      items = page.flatMap(bookmarkMessageItems)
      cursor = nextCursor
      if (!items.some((item) => item.message_id === selectedMessageId)) {
        selectedMessageId = items[0]?.message_id || ''
      }
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      loading = false
    }
  }

  async function loadMore() {
    if (!cursor || loadingMore) {
      return
    }
    loadingMore = true
    try {
      const { items: page, nextCursor } = await listActiveBookmarks(cursor)
      items = [...items, ...page.flatMap(bookmarkMessageItems)]
      cursor = nextCursor
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      loadingMore = false
    }
  }

  function listActiveBookmarks(pageCursor?: string) {
    if (activeFolder === 'all') {
      return andaClient.listBookmarks(pageCursor)
    }
    return andaClient.listBookmarksInFolder(
      activeFolder === 'unfiled' ? 0 : activeFolder,
      pageCursor
    )
  }

  function folderItems(): BookmarkFolder[] {
    return Object.values(folders.folders).sort(
      (left, right) => left.order - right.order || left._id - right._id
    )
  }

  function bookmarkMessageItems(bookmark: Bookmark): BookmarkedMessage[] {
    return (bookmark.messages || [])
      .map((message) => {
        const messageIndex = Number(message.index)
        if (!Number.isInteger(messageIndex) || messageIndex < 0) {
          return null
        }
        return {
          bookmark,
          message_id: `m-${bookmark.conversation}-${messageIndex}`,
          message_index: messageIndex,
          conversation: bookmark.conversation,
          source: bookmark.source,
          role: message.role,
          folder_ids: bookmarkFolderIds(bookmark),
          text: message.text,
          created_at: bookmark.created_at
        } satisfies BookmarkedMessage
      })
      .filter((item): item is BookmarkedMessage => Boolean(item))
      .sort((left, right) => right.message_index - left.message_index)
  }

  function bookmarkFolderIds(bookmark: Bookmark | BookmarkedMessage): number[] {
    return Array.isArray(bookmark.folder_ids) ? bookmark.folder_ids : []
  }

  function folderName(folderId: number): string {
    return folders.folders[String(folderId)]?.name || ''
  }

  function folderCount(folder: ActiveFolder): number {
    if (folder === 'all') {
      return items.length
    }
    if (folder === 'unfiled') {
      return items.filter((item) => bookmarkFolderIds(item).length === 0).length
    }
    return items.filter((item) => bookmarkFolderIds(item).includes(folder)).length
  }

  function selectFolder(folder: ActiveFolder) {
    if (activeFolder === folder) {
      return
    }
    activeFolder = folder
    selectedMessageId = ''
    void loadFirstPage()
  }

  async function createFolder() {
    const name = newFolderName.trim()
    if (!name || creatingFolder) {
      return
    }
    creatingFolder = true
    error = ''
    try {
      folders = await andaClient.createBookmarkFolder(name)
      newFolderName = ''
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      creatingFolder = false
    }
  }

  async function deleteFolder(folderId: number) {
    if (deletingFolderIds.has(folderId)) {
      return
    }
    deletingFolderIds = new Set([...deletingFolderIds, folderId])
    error = ''
    try {
      folders = await andaClient.deleteBookmarkFolder(folderId)
      if (activeFolder === folderId) {
        activeFolder = 'all'
      }
      await loadFirstPage()
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      const next = new Set(deletingFolderIds)
      next.delete(folderId)
      deletingFolderIds = next
    }
  }

  function keepBookmarkInActiveFilter(item: BookmarkedMessage): boolean {
    const ids = bookmarkFolderIds(item)
    if (activeFolder === 'all') {
      return true
    }
    if (activeFolder === 'unfiled') {
      return ids.length === 0
    }
    return ids.includes(activeFolder)
  }

  function replaceBookmark(updated: Bookmark | null) {
    if (!updated) {
      return
    }
    const nextItems = bookmarkMessageItems(updated).filter(keepBookmarkInActiveFilter)
    items = [
      ...items.filter((item) => item.conversation !== updated.conversation),
      ...nextItems
    ].sort(compareBookmarkItems)
  }

  function compareBookmarkItems(left: BookmarkedMessage, right: BookmarkedMessage): number {
    return (
      right.bookmark._id - left.bookmark._id ||
      right.message_index - left.message_index ||
      left.message_id.localeCompare(right.message_id)
    )
  }

  async function addToFolder(bookmark: BookmarkedMessage, event: Event) {
    const select = event.currentTarget as HTMLSelectElement
    const folderId = Number(select.value)
    select.value = ''
    if (!folderId || assigningIds.has(bookmark.message_id)) {
      return
    }
    assigningIds = new Set([...assigningIds, bookmark.message_id])
    error = ''
    try {
      replaceBookmark(await andaClient.addBookmarkToFolder(bookmark.message_id, folderId))
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      const next = new Set(assigningIds)
      next.delete(bookmark.message_id)
      assigningIds = next
    }
  }

  async function removeFromFolder(bookmark: BookmarkedMessage, folderId: number) {
    if (assigningIds.has(bookmark.message_id)) {
      return
    }
    assigningIds = new Set([...assigningIds, bookmark.message_id])
    error = ''
    try {
      replaceBookmark(await andaClient.removeBookmarkFromFolder(bookmark.message_id, folderId))
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      const next = new Set(assigningIds)
      next.delete(bookmark.message_id)
      assigningIds = next
    }
  }

  async function removeBookmark(bookmark: BookmarkedMessage) {
    if (removingIds.has(bookmark.message_id)) {
      return
    }
    removingIds = new Set([...removingIds, bookmark.message_id])
    try {
      const removed = await andaClient.removeBookmark(bookmark.message_id)
      if (removed) {
        items = items.filter((item) => item.message_id !== bookmark.message_id)
      }
    } finally {
      const next = new Set(removingIds)
      next.delete(bookmark.message_id)
      removingIds = next
    }
  }

  async function openSidePanel(bookmark: BookmarkedMessage | null = selectedItem) {
    if (!bookmark) {
      return
    }

    void chrome.storage.local.set({
      [bookmarkJumpRequestStorageKey]: createBookmarkJumpRequest(bookmark)
    })

    if (chrome.sidePanel?.open) {
      try {
        const tab = await chrome.tabs.getCurrent()
        if (typeof tab?.id === 'number') {
          await chrome.sidePanel.open({ tabId: tab.id })
          return
        }
        if (typeof tab?.windowId === 'number') {
          await chrome.sidePanel.open({ windowId: tab.windowId })
          return
        }
      } catch (_error) {
        try {
          const currentWindow = await chrome.windows.getCurrent()
          if (typeof currentWindow?.id === 'number') {
            await chrome.sidePanel.open({ windowId: currentWindow.id })
            return
          }
        } catch (_fallbackError) {
          // Fall through to opening the side panel page as a tab below.
        }
      }
    }

    const url = chrome.runtime.getURL('index.html')
    chrome.tabs.create({ url, active: true }).catch(() => {
      window.open(url, '_blank', 'noopener,noreferrer')
    })
  }

  async function copyMarkdownMessage(bookmark: BookmarkedMessage | null = selectedItem) {
    if (!bookmark || !navigator.clipboard?.writeText) {
      return
    }

    const markdown =
      selectedDetailMessageId === bookmark.message_id && selectedDetailMarkdown
        ? selectedDetailMarkdown
        : await andaClient.getConversationMarkdownForBookmark(bookmark)
    if (!markdown) {
      selectedDetailError = getMessage('bookmarkNotLocated')
      return
    }

    await navigator.clipboard.writeText(markdown)
    copiedMessageId = bookmark.message_id
    window.setTimeout(() => {
      if (copiedMessageId === bookmark.message_id) {
        copiedMessageId = ''
      }
    }, 1200)
  }

  async function loadSelectedDetailMarkdown(bookmark: BookmarkedMessage, messageId: string) {
    try {
      const markdown = await andaClient.getConversationMarkdownForBookmark(bookmark)
      if (selectedDetailMessageId !== messageId) {
        return
      }
      selectedDetailMarkdown = markdown
      selectedDetailError = markdown ? '' : getMessage('bookmarkNotLocated')
    } catch (err) {
      if (selectedDetailMessageId === messageId) {
        selectedDetailError = errorToMessage(err)
      }
    } finally {
      if (selectedDetailMessageId === messageId) {
        selectedDetailLoading = false
      }
    }
  }

  function previewText(bookmark: BookmarkedMessage): string {
    return bookmark.text.trim().replace(/\s+/g, ' ')
  }

  function timeLabel(value: number | undefined): string {
    if (!value) {
      return ''
    }
    const date = new Date(value)
    if (Number.isNaN(date.getTime())) {
      return ''
    }
    return date.toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    })
  }
</script>

<div
  class="grid h-full min-h-0 grid-cols-[10rem_minmax(26rem,36rem)_minmax(30rem,1fr)] bg-background"
>
  <aside class="flex min-h-0 flex-col border-r bg-muted/20">
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
      <button
        type="button"
        class={`flex min-w-0 items-center justify-between gap-2 rounded-md px-2.5 py-2 text-left text-sm transition ${
          activeFolder === 'all'
            ? 'bg-background text-foreground shadow-xs'
            : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
        }`}
        onclick={() => selectFolder('all')}
      >
        <span class="flex min-w-0 items-center gap-2">
          <BookmarkIcon class="size-3.5 shrink-0" />
          <span class="truncate">{getMessage('allBookmarks')}</span>
        </span>
        <span class="text-xs text-muted-foreground">{folderCount('all')}</span>
      </button>
      <button
        type="button"
        class={`flex min-w-0 items-center justify-between gap-2 rounded-md px-2.5 py-2 text-left text-sm transition ${
          activeFolder === 'unfiled'
            ? 'bg-background text-foreground shadow-xs'
            : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
        }`}
        onclick={() => selectFolder('unfiled')}
      >
        <span class="flex min-w-0 items-center gap-2">
          <Folder class="size-3.5 shrink-0" />
          <span class="truncate">{getMessage('unfiledBookmarks')}</span>
        </span>
        <span class="text-xs text-muted-foreground">{folderCount('unfiled')}</span>
      </button>

      {#each folderItems() as folder (folder._id)}
        <div
          class={`group/folder grid grid-cols-[minmax(0,1fr)_auto] overflow-hidden rounded-md ${
            activeFolder === folder._id
              ? 'bg-background text-foreground shadow-xs'
              : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
          }`}
        >
          <button
            type="button"
            class="flex min-w-0 items-center justify-between gap-2 px-2.5 py-2 text-left text-sm"
            title={folder.name}
            onclick={() => selectFolder(folder._id)}
          >
            <span class="flex min-w-0 items-center gap-2">
              <Folder class="size-3.5 shrink-0" />
              <span class="truncate">{folder.name}</span>
            </span>
            <span class="text-xs text-muted-foreground">{folderCount(folder._id)}</span>
          </button>
          <button
            type="button"
            class="grid size-8 place-items-center text-muted-foreground hover:text-amber-700 disabled:opacity-50"
            disabled={deletingFolderIds.has(folder._id)}
            aria-label={getMessage('deleteBookmarkFolder')}
            title={getMessage('deleteBookmarkFolder')}
            onclick={() => deleteFolder(folder._id)}
          >
            {#if deletingFolderIds.has(folder._id)}
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
        disabled={creatingFolder || !newFolderName.trim()}
        aria-label={getMessage('createBookmarkFolder')}
        title={getMessage('createBookmarkFolder')}
      >
        {#if creatingFolder}
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
          onclick={loadFirstPage}
          disabled={loading}
        >
          <RefreshCw class={`size-3.5 ${loading ? 'animate-spin' : ''}`} />
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
        />
      </label>
    </div>

    <div class="scrollbar-slim min-h-0 flex-1 overflow-y-auto">
      {#if loading}
        <div class="grid h-full min-h-80 place-items-center text-sm text-muted-foreground">
          <span class="flex items-center gap-2">
            <LoaderCircle class="size-4 animate-spin" />
            {getMessage('loading')}
          </span>
        </div>
      {:else if error}
        <div
          class="grid h-full min-h-80 place-items-center px-6 text-center text-sm text-amber-700"
        >
          {error}
        </div>
      {:else if visibleItems.length === 0}
        <div
          class="grid h-full min-h-80 place-items-center px-6 text-center text-sm text-muted-foreground"
        >
          <div class="grid max-w-72 gap-2">
            <BookmarkIcon class="mx-auto size-7" />
            <div class="font-medium">{getMessage('bookmarksEmpty')}</div>
          </div>
        </div>
      {:else}
        <div class="divide-y">
          {#each visibleItems as bookmark (bookmark.message_id)}
            <article
              class={`group grid gap-2 px-4 py-3 transition hover:bg-muted/35 ${
                selectedItem?.message_id === bookmark.message_id ? 'bg-muted/45' : ''
              }`}
            >
              <div class="flex min-w-0 items-start justify-between gap-3">
                <button
                  type="button"
                  class="min-w-0 cursor-default text-left"
                  title={getMessage('bookmarkJump')}
                  onclick={() => (selectedMessageId = bookmark.message_id)}
                >
                  <p class="line-clamp-2 text-sm leading-relaxed wrap-break-word">
                    {previewText(bookmark)}
                  </p>
                  <div
                    class="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-muted-foreground"
                  >
                    {#if bookmark.source}
                      <span class="max-w-64 truncate" title={bookmark.source}
                        >{bookmark.source}</span
                      >
                    {/if}
                    {#if timeLabel(bookmark.created_at)}
                      <span>{timeLabel(bookmark.created_at)}</span>
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
                  disabled={removingIds.has(bookmark.message_id)}
                  aria-label={getMessage('removeBookmark')}
                  title={getMessage('removeBookmark')}
                  onclick={(event) => {
                    event.stopPropagation()
                    removeBookmark(bookmark)
                  }}
                >
                  {#if removingIds.has(bookmark.message_id)}
                    <LoaderCircle class="size-4 animate-spin" />
                  {:else}
                    <Trash2 class="size-4" />
                  {/if}
                </button>
              </div>

              <div class="flex min-w-0 flex-wrap items-center gap-1">
                {#each bookmarkFolderIds(bookmark) as folderId (folderId)}
                  {#if folderName(folderId)}
                    <span
                      class="inline-flex h-6 max-w-40 items-center gap-1 rounded-md border bg-background px-2 text-[11px] text-muted-foreground"
                    >
                      <Folder class="size-3 shrink-0" />
                      <span class="truncate">{folderName(folderId)}</span>
                      <button
                        type="button"
                        class="ml-0.5 rounded-sm text-muted-foreground transition hover:text-amber-700 disabled:opacity-50"
                        disabled={assigningIds.has(bookmark.message_id)}
                        aria-label={getMessage('removeFromBookmarkFolder')}
                        title={getMessage('removeFromBookmarkFolder')}
                        onclick={(event) => {
                          event.stopPropagation()
                          removeFromFolder(bookmark, folderId)
                        }}
                      >
                        <X class="size-3" />
                      </button>
                    </span>
                  {/if}
                {/each}
                {#if folderItems().length > bookmarkFolderIds(bookmark).length}
                  <select
                    class="h-6 max-w-44 rounded-md border bg-background px-2 text-[11px] text-muted-foreground outline-none transition focus:border-ring disabled:opacity-50"
                    disabled={assigningIds.has(bookmark.message_id)}
                    aria-label={getMessage('addToBookmarkFolder')}
                    title={getMessage('addToBookmarkFolder')}
                    onclick={(event) => event.stopPropagation()}
                    onchange={(event) => addToFolder(bookmark, event)}
                  >
                    <option value="">{getMessage('addToBookmarkFolder')}</option>
                    {#each folderItems().filter((folder) => !bookmarkFolderIds(bookmark).includes(folder._id)) as folder (folder._id)}
                      <option value={folder._id}>{folder.name}</option>
                    {/each}
                  </select>
                {/if}
              </div>
            </article>
          {/each}
        </div>

        {#if cursor}
          <div class="flex justify-center border-t py-3">
            <button
              type="button"
              class={buttonClass('outline', 'sm')}
              disabled={loadingMore}
              onclick={loadMore}
            >
              {#if loadingMore}
                <LoaderCircle class="size-3.5 animate-spin" />
              {/if}
              {getMessage('loadMore')}
            </button>
          </div>
        {/if}
      {/if}
    </div>
  </section>

  <aside class="flex min-h-0 min-w-0 flex-col border-l bg-muted/20">
    <div class="flex items-start justify-between gap-3 border-b px-4 py-3">
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-sm font-bold">
          <BrainCircuit class="size-4 text-emerald-800" />
          {getMessage('bookmarkContextTitle')}
        </div>
        <p class="mt-1 text-xs text-muted-foreground">{getMessage('bookmarkContextSubtitle')}</p>
      </div>
      {#if selectedItem}
        <button
          type="button"
          class={buttonClass('outline', 'icon-sm', 'shrink-0')}
          disabled={selectedDetailLoading}
          aria-label={getMessage(
            copiedMessageId === selectedItem.message_id
              ? 'bookmarkCopiedMarkdown'
              : 'bookmarkCopyMarkdown'
          )}
          title={getMessage(
            copiedMessageId === selectedItem.message_id
              ? 'bookmarkCopiedMarkdown'
              : 'bookmarkCopyMarkdown'
          )}
          onclick={() => copyMarkdownMessage(selectedItem)}
        >
          {#if selectedDetailLoading}
            <LoaderCircle class="size-4 animate-spin" />
          {:else if copiedMessageId === selectedItem.message_id}
            <Check class="size-4" />
          {:else}
            <Copy class="size-4" />
          {/if}
        </button>
      {/if}
    </div>

    <div class="scrollbar-slim grid min-h-0 flex-1 content-start gap-4 overflow-y-auto p-4">
      {#if selectedItem}
        <section class="grid gap-2 border-b pb-4">
          <div class="text-xs font-medium text-muted-foreground">
            {getMessage('bookmarkSelected')}
          </div>
          {#if selectedDetailLoading}
            <div class="flex items-center gap-2 text-sm text-muted-foreground">
              <LoaderCircle class="size-4 animate-spin" />
              {getMessage('loading')}
            </div>
          {:else if selectedDetailError}
            <p class="text-sm leading-relaxed text-amber-700 wrap-break-word">
              {selectedDetailError}
            </p>
          {:else}
            <div class="md-content w-full min-w-0 text-pretty wrap-break-word">
              {@html selectedDetailHtml}
            </div>
          {/if}
          <div class="grid gap-1 text-xs text-muted-foreground">
            {#if selectedItem.source}
              <span class="truncate">{selectedItem.source}</span>
            {/if}
            <span>{timeLabel(selectedItem.created_at) || selectedItem.message_id}</span>
          </div>
          <button
            type="button"
            class={buttonClass('outline', 'sm', 'mt-1 justify-between')}
            onclick={() => openSidePanel(selectedItem)}
          >
            <span class="truncate">{getMessage('bookmarkOpenChat')}</span>
            <ExternalLink class="size-3.5" />
          </button>
        </section>

        <section class="grid gap-2 border-b pb-4">
          <div class="flex items-center gap-2 text-xs font-semibold">
            <Folder class="size-3.5" />
            {getMessage('bookmarkFolders')}
          </div>
          <div class="flex flex-wrap gap-1">
            {#each bookmarkFolderIds(selectedItem) as folderId (folderId)}
              {#if folderName(folderId)}
                <span
                  class="rounded-md border bg-background px-2 py-1 text-[11px] text-muted-foreground"
                >
                  {folderName(folderId)}
                </span>
              {/if}
            {/each}
            {#if bookmarkFolderIds(selectedItem).length === 0}
              <span class="text-xs text-muted-foreground">{getMessage('unfiledBookmarks')}</span>
            {/if}
          </div>
        </section>

        <section class="grid gap-2 border-b pb-4">
          <div class="flex items-center gap-2 text-xs font-semibold">
            <BrainCircuit class="size-3.5" />
            {getMessage('bookmarkLinkedBrain')}
          </div>
          <div class="flex flex-wrap gap-1">
            {#each selectedDetailConceptTokens as token}
              <span
                class="rounded-md border bg-background px-2 py-1 text-[11px] text-muted-foreground"
              >
                {token}
              </span>
            {/each}
            {#if !selectedDetailLoading && selectedDetailConceptTokens.length === 0}
              <span class="text-xs text-muted-foreground">{getMessage('bookmarkNotLocated')}</span>
            {/if}
          </div>
        </section>
      {:else}
        <div class="grid min-h-72 place-items-center text-center text-sm text-muted-foreground">
          {getMessage('bookmarksEmpty')}
        </div>
      {/if}
    </div>
  </aside>
</div>
