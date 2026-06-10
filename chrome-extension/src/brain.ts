import { mount } from 'svelte'
import './app.css'
import BrainApp from './BrainApp.svelte'

const target = document.getElementById('app')
if (!target) {
  throw new Error('Could not find app container')
}

mount(BrainApp, { target })
