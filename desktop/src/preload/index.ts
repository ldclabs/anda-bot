import { contextBridge, ipcRenderer } from 'electron'
import type { DesktopBridge, NativeEvent } from '../shared/contract'

const api: DesktopBridge = {
  browser: (request) => ipcRenderer.invoke('anda:browser', request),
  git: (request) => ipcRenderer.invoke('anda:git', request),
  terminal: (request) => ipcRenderer.invoke('anda:terminal', request),
  bootstrap: () => ipcRenderer.invoke('anda:bootstrap'),
  connect: () => ipcRenderer.invoke('anda:connect'),
  control: (action) => ipcRenderer.invoke('anda:control', action),
  rpc: (method, params, submissionId) =>
    ipcRenderer.invoke('anda:rpc', method, params, submissionId),
  config: (method, content, revision) =>
    ipcRenderer.invoke('anda:config', method, content, revision),
  preferences: (patch) => ipcRenderer.invoke('anda:preferences', patch),
  storageGet: (keys) => ipcRenderer.invoke('anda:storage:get', keys),
  storageSet: (items) => ipcRenderer.invoke('anda:storage:set', items),
  chooseWorkspace: () => ipcRenderer.invoke('anda:workspace'),
  chooseBinary: () => ipcRenderer.invoke('anda:binary'),
  notify: (source, title, body) => ipcRenderer.invoke('anda:notify', source, title, body),
  acknowledgeSubmission: (id) => ipcRenderer.invoke('anda:submission:acknowledge', id),
  readSubmission: (id) => ipcRenderer.invoke('anda:submission:read', id),
  openExternal: (url) => ipcRenderer.invoke('anda:external', url),
  showLogs: () => ipcRenderer.invoke('anda:logs'),
  printHtml: (html) => ipcRenderer.invoke('anda:print', html),
  checkUpdate: () => ipcRenderer.invoke('anda:update'),
  onEvent: (listener) => {
    const handler = (_event: Electron.IpcRendererEvent, data: NativeEvent) => listener(data)
    ipcRenderer.on('anda:event', handler)
    return () => ipcRenderer.removeListener('anda:event', handler)
  }
}
contextBridge.exposeInMainWorld('anda', api)
