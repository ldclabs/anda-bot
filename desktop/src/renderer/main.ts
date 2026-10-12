import { mount } from 'svelte'
import './style.css'
import App from './App.svelte'
import PanelWindow from './PanelWindow.svelte'
import { DesktopClient } from './client.svelte'
import { PanelClient } from './panel-client.svelte'
import { isPanelTab } from '../shared/panels'

const target = document.getElementById('app')!
// A workbench panel moved into a window of its own loads this page as `?panel=<tab>`.
if (isPanelTab(new URLSearchParams(location.search).get('panel'))) {
  const client = new PanelClient()
  mount(PanelWindow, { target, props: { client } })
  void client.init().catch((error) => client.fail(error))
} else {
  const client = new DesktopClient()
  mount(App, { target, props: { client } })
  void client.init().catch((error) => {
    client.fail(error)
    client.ready = true
  })
}
