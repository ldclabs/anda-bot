import { mount } from 'svelte'
import { initI18n, watchUiLanguage } from '$lib/i18n'
import './app.css'
import DashboardApp from './DashboardApp.svelte'

const target = document.getElementById('app')
if (!target) {
  throw new Error('Could not find app container')
}

watchUiLanguage(() => {
  window.location.reload()
})

initI18n().finally(() => {
  mount(DashboardApp, { target })
})
