import { mount } from 'svelte'
import './app.css'
import ConfigApp from './ConfigApp.svelte'

const target = document.getElementById('app')
if (!target) {
  throw new Error('Could not find app container')
}

mount(ConfigApp, { target })
