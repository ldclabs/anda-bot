import { mount } from 'svelte'
import './style.css'
import App from './App.svelte'
import { DesktopClient } from './client.svelte'

const client = new DesktopClient()
mount(App, { target: document.getElementById('app')!, props: { client } })
void client.init().catch((error) => {
  client.fail(error)
  client.ready = true
})
