<script lang="ts">
  /**
   * Language picker for General settings: {@link DropdownMenu} over the UI
   * languages, each named in itself.
   *
   * Picking a language is not an assignment: it is saved, then the window
   * reloads so every string switches. `client.preferences.language` is
   * therefore not the menu's to write, and the menu gets `onSelect` rather
   * than `bind:value`.
   */
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { UI_LANGUAGE_NAMES, UI_LANGUAGES, type UiLanguage } from '$lib/i18n'
  import type { DesktopClient } from './client.svelte'
  import { label } from './labels'

  let { client }: { client: DesktopClient } = $props()

  const items = UI_LANGUAGES.map((code) => ({ value: code, label: UI_LANGUAGE_NAMES[code] }))

  async function choose(language: UiLanguage) {
    try {
      await client.savePreferences({ language })
    } catch (error) {
      client.fail(error)
      return
    }
    location.reload()
  }
</script>

<DropdownMenu
  {items}
  value={client.preferences.language as UiLanguage}
  onSelect={(language) => void choose(language)}
  ariaLabel={label(client.preferences.language, 'language')}
  align="end"
/>
