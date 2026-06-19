<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import type {
    Bookmark,
    BookmarkFolder,
    BookmarkFolders,
    BookmarkedMessage
  } from '$lib/anda/client/types'
  import {
    buttonClass,
    dialogContentClass,
    dialogDescriptionClass,
    dialogOverlayClass,
    inputClass
  } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { Bookmark as BookmarkIcon, Folder, LoaderCircle, Plus, Trash2, X } from '@lucide/svelte'
  import { Dialog } from 'bits-ui'

  type ActiveFolder = 'all' | 'unfiled' | number

  let {
    open = $bindable(false),
    onJump
  }: { open?: boolean; onJump: (bookmark: BookmarkedMessage) => boolean | Promise<boolean> } =
    $props()

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
  let cursor = $state<string | null>(null)
  let loading = $state(false)
  let loadingMore = $state(false)
  let creatingFolder = $state(false)
  let removingIds = $state(new Set<string>())
  let deletingFolderIds = $state(new Set<number>())
  let assigningIds = $state(new Set<string>())
  let error = $state('')

  // Reload a fresh first page each time the panel opens.
  $effect(() => {
    if (open) {
      void loadFirstPage()
    }
  })

  async function loadFirstPage() {
    loading = true
    error = ''
    try {
      folders = await andaClient.listBookmarkFolders()
      const { items: page, nextCursor } = await listActiveBookmarks()
      items = page.flatMap(bookmarkMessageItems)
      cursor = nextCursor
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

  function isActiveFolder(folder: ActiveFolder): boolean {
    return activeFolder === folder
  }

  function folderButtonClass(folder: ActiveFolder): string {
    return buttonClass(
      isActiveFolder(folder) ? 'secondary' : 'outline',
      'xs',
      'max-w-44 justify-start'
    )
  }

  function selectFolder(folder: ActiveFolder) {
    if (activeFolder === folder) {
      return
    }
    activeFolder = folder
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

  async function jumpTo(bookmark: BookmarkedMessage) {
    try {
      const located = await onJump(bookmark)
      if (located) {
        open = false
      }
    } catch (err) {
      error = errorToMessage(err)
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

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class={dialogOverlayClass()} />
    <Dialog.Content
      class={dialogContentClass(
        'flex max-h-[min(90vh,46rem)] min-h-0 flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl'
      )}
      aria-label={getMessage('bookmarks')}
    >
      <Dialog.Close>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class={buttonClass('ghost', 'icon-sm', 'absolute top-4 right-4 z-10')}
          >
            <X class="size-4" />
            <span class="sr-only">{getMessage('close')}</span>
          </button>
        {/snippet}
      </Dialog.Close>

      <div class="flex shrink-0 flex-col gap-2 border-b bg-muted/35 px-5 py-4 pr-12">
        <Dialog.Title class="flex min-w-0 items-center gap-2 text-base font-bold">
          <BookmarkIcon class="size-4 shrink-0 text-emerald-800" />
          <span class="truncate">{getMessage('bookmarks')}</span>
        </Dialog.Title>
        <Dialog.Description class={dialogDescriptionClass('text-xs leading-relaxed')}>
          {getMessage('bookmarksDescription')}
        </Dialog.Description>
      </div>

      <div class="scrollbar-slim flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4">
        <div class="flex shrink-0 flex-col gap-2 rounded-md border bg-background p-2">
          <div class="scrollbar-slim flex min-w-0 gap-1 overflow-x-auto pb-0.5">
            <button
              type="button"
              class={folderButtonClass('all')}
              aria-pressed={activeFolder === 'all'}
              onclick={() => selectFolder('all')}
            >
              <BookmarkIcon class="size-3.5" />
              <span class="truncate">{getMessage('allBookmarks')}</span>
            </button>
            <button
              type="button"
              class={folderButtonClass('unfiled')}
              aria-pressed={activeFolder === 'unfiled'}
              onclick={() => selectFolder('unfiled')}
            >
              <Folder class="size-3.5" />
              <span class="truncate">{getMessage('unfiledBookmarks')}</span>
            </button>
            {#each folderItems() as folder (folder._id)}
              <div class="flex shrink-0 overflow-hidden rounded-md border bg-background">
                <button
                  type="button"
                  class={buttonClass(
                    isActiveFolder(folder._id) ? 'secondary' : 'ghost',
                    'xs',
                    'max-w-40 rounded-r-none border-0 justify-start'
                  )}
                  aria-pressed={activeFolder === folder._id}
                  title={folder.name}
                  onclick={() => selectFolder(folder._id)}
                >
                  <Folder class="size-3.5" />
                  <span class="truncate">{folder.name}</span>
                </button>
                <button
                  type="button"
                  class={buttonClass(
                    'ghost',
                    'icon-xs',
                    'rounded-l-none text-muted-foreground hover:text-amber-700'
                  )}
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
          </div>

          <form
            class="flex min-w-0 gap-2"
            onsubmit={(event) => (event.preventDefault(), createFolder())}
          >
            <input
              class={inputClass('h-8 text-sm')}
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
        </div>

        {#if loading}
          <div class="m-auto flex items-center gap-2 py-10 text-sm text-muted-foreground">
            <LoaderCircle class="size-4 animate-spin" />
            <span>{getMessage('loading')}</span>
          </div>
        {:else if error}
          <div class="m-auto py-10 text-center text-sm text-amber-700">{error}</div>
        {:else if items.length === 0}
          <div
            class="m-auto grid max-w-64 place-items-center gap-2 py-10 text-center text-muted-foreground"
          >
            <BookmarkIcon class="size-6" />
            <div class="text-sm font-medium">{getMessage('bookmarksEmpty')}</div>
          </div>
        {:else}
          {#each items as bookmark (bookmark.message_id)}
            <div
              class="group flex items-start gap-2 rounded-lg border bg-background p-3 shadow-xs transition hover:bg-muted/40"
            >
              <div class="min-w-0 flex-1">
                <button
                  type="button"
                  class="w-full min-w-0 text-left"
                  title={getMessage('bookmarkJump')}
                  onclick={() => jumpTo(bookmark)}
                >
                  <p class="line-clamp-2 text-sm leading-relaxed wrap-break-word">
                    {previewText(bookmark)}
                  </p>
                  <div
                    class="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-muted-foreground"
                  >
                    {#if bookmark.source}
                      <span class="max-w-48 truncate" title={bookmark.source}
                        >{bookmark.source}</span
                      >
                    {/if}
                    {#if timeLabel(bookmark.created_at)}
                      <span>{timeLabel(bookmark.created_at)}</span>
                    {/if}
                  </div>
                </button>
                <div class="mt-2 flex min-w-0 flex-wrap items-center gap-1">
                  {#each bookmarkFolderIds(bookmark) as folderId (folderId)}
                    {#if folderName(folderId)}
                      <span
                        class="inline-flex h-6 max-w-36 items-center gap-1 rounded-md border bg-muted/45 px-2 text-[11px] text-muted-foreground"
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
                      class="h-6 max-w-40 rounded-md border bg-background px-2 text-[11px] text-muted-foreground outline-none transition focus:border-ring disabled:opacity-50"
                      disabled={assigningIds.has(bookmark.message_id)}
                      aria-label={getMessage('addToBookmarkFolder')}
                      title={getMessage('addToBookmarkFolder')}
                      onchange={(event) => addToFolder(bookmark, event)}
                    >
                      <option value="">{getMessage('addToBookmarkFolder')}</option>
                      {#each folderItems().filter((folder) => !bookmarkFolderIds(bookmark).includes(folder._id)) as folder (folder._id)}
                        <option value={folder._id}>{folder.name}</option>
                      {/each}
                    </select>
                  {/if}
                </div>
              </div>

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
                onclick={() => removeBookmark(bookmark)}
              >
                {#if removingIds.has(bookmark.message_id)}
                  <LoaderCircle class="size-4 animate-spin" />
                {:else}
                  <Trash2 class="size-4" />
                {/if}
              </button>
            </div>
          {/each}

          {#if cursor}
            <div class="flex justify-center py-1">
              <button
                type="button"
                class={buttonClass('outline', 'sm', 'shadow-sm')}
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
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
