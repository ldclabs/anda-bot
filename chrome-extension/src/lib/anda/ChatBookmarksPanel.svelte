<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import type { BookmarkedMessage } from '$lib/anda/client/types'
  import {
    BookmarkBrowser,
    bookmarkFolderIds,
    bookmarkPreviewText,
    type ActiveFolder
  } from '$lib/anda/bookmarks/browser.svelte'
  import {
    buttonClass,
    dialogContentClass,
    dialogDescriptionClass,
    dialogOverlayClass,
    inputClass
  } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { formatTimestamp } from '$lib/utils/format'
  import { Bookmark as BookmarkIcon, Folder, LoaderCircle, Plus, Trash2, X } from '@lucide/svelte'
  import { Dialog } from 'bits-ui'

  let {
    open = $bindable(false),
    onJump
  }: { open?: boolean; onJump: (bookmark: BookmarkedMessage) => boolean | Promise<boolean> } =
    $props()

  const browser = new BookmarkBrowser(andaClient.bookmarks)
  let newFolderName = $state('')

  // Reload a fresh first page each time the panel opens.
  $effect(() => {
    if (open) {
      void browser.load()
    }
  })

  function folderButtonClass(folder: ActiveFolder): string {
    return buttonClass(
      browser.activeFolder === folder ? 'secondary' : 'outline',
      'xs',
      'max-w-44 justify-start'
    )
  }

  async function createFolder() {
    if (await browser.createFolder(newFolderName)) {
      newFolderName = ''
    }
  }

  function addToFolder(bookmark: BookmarkedMessage, event: Event) {
    const select = event.currentTarget as HTMLSelectElement
    const folderId = Number(select.value)
    select.value = ''
    if (folderId) {
      void browser.addToFolder(bookmark.message_id, folderId)
    }
  }

  async function jumpTo(bookmark: BookmarkedMessage) {
    try {
      if (await onJump(bookmark)) {
        open = false
      }
    } catch (err) {
      browser.error = errorToMessage(err)
    }
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
              aria-pressed={browser.activeFolder === 'all'}
              onclick={() => browser.selectFolder('all')}
            >
              <BookmarkIcon class="size-3.5" />
              <span class="truncate">{getMessage('allBookmarks')}</span>
            </button>
            <button
              type="button"
              class={folderButtonClass('unfiled')}
              aria-pressed={browser.activeFolder === 'unfiled'}
              onclick={() => browser.selectFolder('unfiled')}
            >
              <Folder class="size-3.5" />
              <span class="truncate">{getMessage('unfiledBookmarks')}</span>
            </button>
            {#each browser.folderList as folder (folder._id)}
              <div class="flex shrink-0 overflow-hidden rounded-md border bg-background">
                <button
                  type="button"
                  class={buttonClass(
                    browser.activeFolder === folder._id ? 'secondary' : 'ghost',
                    'xs',
                    'max-w-40 rounded-r-none border-0 justify-start'
                  )}
                  aria-pressed={browser.activeFolder === folder._id}
                  title={folder.name}
                  onclick={() => browser.selectFolder(folder._id)}
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
        </div>

        {#if browser.loading}
          <div class="m-auto flex items-center gap-2 py-10 text-sm text-muted-foreground">
            <LoaderCircle class="size-4 animate-spin" />
            <span>{getMessage('loading')}</span>
          </div>
        {:else if browser.error}
          <div class="m-auto py-10 text-center text-sm text-amber-700">{browser.error}</div>
        {:else if browser.items.length === 0}
          <div
            class="m-auto grid max-w-64 place-items-center gap-2 py-10 text-center text-muted-foreground"
          >
            <BookmarkIcon class="size-6" />
            <div class="text-sm font-medium">{getMessage('bookmarksEmpty')}</div>
          </div>
        {:else}
          {#each browser.items as bookmark (bookmark.message_id)}
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
                    {bookmarkPreviewText(bookmark)}
                  </p>
                  <div
                    class="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-muted-foreground"
                  >
                    {#if bookmark.source}
                      <span class="max-w-48 truncate" title={bookmark.source}
                        >{bookmark.source}</span
                      >
                    {/if}
                    {#if formatTimestamp(bookmark.created_at)}
                      <span>{formatTimestamp(bookmark.created_at)}</span>
                    {/if}
                  </div>
                </button>
                <div class="mt-2 flex min-w-0 flex-wrap items-center gap-1">
                  {#each bookmarkFolderIds(bookmark) as folderId (folderId)}
                    {#if browser.folderName(folderId)}
                      <span
                        class="inline-flex h-6 max-w-36 items-center gap-1 rounded-md border bg-muted/45 px-2 text-[11px] text-muted-foreground"
                      >
                        <Folder class="size-3 shrink-0" />
                        <span class="truncate">{browser.folderName(folderId)}</span>
                        <button
                          type="button"
                          class="ml-0.5 rounded-sm text-muted-foreground transition hover:text-amber-700 disabled:opacity-50"
                          disabled={browser.isAssigning(bookmark.message_id)}
                          aria-label={getMessage('removeFromBookmarkFolder')}
                          title={getMessage('removeFromBookmarkFolder')}
                          onclick={(event) => {
                            event.stopPropagation()
                            browser.removeFromFolder(bookmark.message_id, folderId)
                          }}
                        >
                          <X class="size-3" />
                        </button>
                      </span>
                    {/if}
                  {/each}
                  {#if browser.folderList.length > bookmarkFolderIds(bookmark).length}
                    <select
                      class="h-6 max-w-40 rounded-md border bg-background px-2 text-[11px] text-muted-foreground outline-none transition focus:border-ring disabled:opacity-50"
                      disabled={browser.isAssigning(bookmark.message_id)}
                      aria-label={getMessage('addToBookmarkFolder')}
                      title={getMessage('addToBookmarkFolder')}
                      onchange={(event) => addToFolder(bookmark, event)}
                    >
                      <option value="">{getMessage('addToBookmarkFolder')}</option>
                      {#each browser.folderList.filter((folder) => !bookmarkFolderIds(bookmark).includes(folder._id)) as folder (folder._id)}
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
          {/each}

          {#if browser.hasMore}
            <div class="flex justify-center py-1">
              <button
                type="button"
                class={buttonClass('outline', 'sm', 'shadow-sm')}
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
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
