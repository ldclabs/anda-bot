import { mount } from 'svelte'
import { initI18n } from '$lib/i18n'
import './app.css'
import BrainApp from './BrainApp.svelte'

const target = document.getElementById('app')
if (!target) {
  throw new Error('Could not find app container')
}

initI18n().finally(() => {
  mount(BrainApp, { target })
})
