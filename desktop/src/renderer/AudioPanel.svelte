<script lang="ts">
  import { onMount } from 'svelte'
  import type { DesktopClient } from './client.svelte'
  import type { VoiceRecordingInput, TtsToolOutput } from '$lib/anda/client/types'
  import { bytesToBase64, base64ToBytes } from '$lib/utils/base64'
  import { testTone } from './audio-test'
  import { wb } from './workbench-labels'
  let { client }: { client: DesktopClient } = $props()
  const t = (key: Parameters<typeof wb>[1]) => wb(client.preferences.language, key)
  let devices = $state<MediaDeviceInfo[]>([])
  let input = $state('default'),
    output = $state('default'),
    error = $state(''),
    transcript = $state('')
  let recording = $state(false),
    busy = $state(false),
    seconds = $state(0),
    level = $state(0),
    recordingUrl = $state('')
  let recorder: MediaRecorder | undefined,
    stream: MediaStream | undefined,
    context: AudioContext | undefined
  let frame = 0,
    timer: ReturnType<typeof setTimeout> | undefined
  let audio: HTMLAudioElement
  let clip: VoiceRecordingInput | undefined
  let generation = 0,
    disposed = false
  const urls = new Set<string>()
  function fail(e: unknown) {
    if (!disposed) error = e instanceof Error ? e.message : String(e)
  }
  async function refreshDevices() {
    try {
      devices = await navigator.mediaDevices.enumerateDevices()
    } catch (e) {
      fail(e)
    }
  }
  function stop() {
    generation++
    busy = false
    audio?.pause()
    if (recorder?.state === 'recording') recorder.stop()
    stream?.getTracks().forEach((t) => t.stop())
    stream = undefined
    cancelAnimationFrame(frame)
    clearTimeout(timer)
    void context?.close().catch(() => {})
    context = undefined
    recording = false
    level = 0
  }
  async function record() {
    stop()
    error = ''
    seconds = 0
    busy = true
    const epoch = generation
    try {
      const captured = await navigator.mediaDevices.getUserMedia({
        audio: {
          ...(input !== 'default' ? { deviceId: { exact: input } } : {}),
          echoCancellation: true
        }
      })
      if (disposed || epoch !== generation) {
        captured.getTracks().forEach((t) => t.stop())
        return
      }
      stream = captured
      void refreshDevices()
      const mime = ['audio/webm;codecs=opus', 'audio/webm', 'audio/mp4'].find((type) =>
        MediaRecorder.isTypeSupported(type)
      )
      recorder = new MediaRecorder(captured, mime ? { mimeType: mime } : {})
      const chunks: Blob[] = []
      let size = 0
      recorder.ondataavailable = (event) => {
        chunks.push(event.data)
        size += event.data.size
        if (size > 10 * 1024 * 1024) stop()
      }
      recorder.onerror = () => {
        fail('Audio recording failed')
        stop()
      }
      captured.getAudioTracks()[0].onended = () => {
        if (recording) {
          fail('Microphone disconnected')
          stop()
        }
      }
      recorder.onstop = async () => {
        if (disposed || generation > epoch + 1) return
        const blob = new Blob(chunks, { type: mime || 'audio/webm' })
        if (!blob.size) {
          fail('No audio was recorded')
          return
        }
        if (recordingUrl) {
          URL.revokeObjectURL(recordingUrl)
          urls.delete(recordingUrl)
        }
        recordingUrl = URL.createObjectURL(blob)
        urls.add(recordingUrl)
        clip = {
          ttsEnabled: false,
          fileName: mime?.includes('mp4') ? 'desktop-test.m4a' : 'desktop-test.webm',
          mimeType: blob.type,
          size: blob.size,
          audioBase64: bytesToBase64(new Uint8Array(await blob.arrayBuffer()))
        }
        audio.src = recordingUrl
      }
      context = new AudioContext()
      const analyser = context.createAnalyser()
      analyser.fftSize = 256
      context.createMediaStreamSource(captured).connect(analyser)
      const data = new Uint8Array(256),
        started = performance.now()
      const meter = () => {
        if (!recording) return
        analyser.getByteTimeDomainData(data)
        level = Math.sqrt(data.reduce((n, v) => n + ((v - 128) / 128) ** 2, 0) / data.length)
        seconds = (performance.now() - started) / 1000
        frame = requestAnimationFrame(meter)
      }
      busy = false
      recording = true
      recorder.start(250)
      meter()
      timer = setTimeout(stop, 30000)
    } catch (e) {
      stop()
      fail(e)
    }
  }
  async function play(url: string) {
    audio.pause()
    audio.src = url
    if ('setSinkId' in audio) await audio.setSinkId(output)
    await audio.play()
  }
  async function tone() {
    stop()
    error = ''
    const url = URL.createObjectURL(new Blob([testTone()], { type: 'audio/wav' }))
    urls.add(url)
    try {
      await play(url)
    } catch (e) {
      fail(e)
    }
  }
  async function transcribe() {
    if (!clip) return
    const epoch = ++generation
    busy = true
    error = ''
    try {
      const result = await client.voice.transcribe(clip)
      if (epoch === generation && !disposed) transcript = result.text
    } catch (e) {
      if (epoch === generation) fail(e)
    } finally {
      if (epoch === generation) busy = false
    }
  }
  async function speak() {
    stop()
    const epoch = generation
    busy = true
    error = ''
    try {
      const result = await client.toolCall<TtsToolOutput>('synthesize_speech', {
        text: transcript,
        artifact_name: `desktop-audio-test-${Date.now()}`
      })
      if (epoch !== generation || disposed) return
      const resource = result.artifacts?.find((r) => r.mime_type?.startsWith('audio/') && r.blob)
      if (!resource?.blob) throw new Error('No playable audio returned')
      const url = URL.createObjectURL(
        new Blob([base64ToBytes(resource.blob)], { type: resource.mime_type })
      )
      urls.add(url)
      await play(url)
    } catch (e) {
      if (epoch === generation) fail(e)
    } finally {
      if (epoch === generation) busy = false
    }
  }
  onMount(() => {
    void refreshDevices()
    navigator.mediaDevices.addEventListener('devicechange', refreshDevices)
    return () => {
      disposed = true
      stop()
      navigator.mediaDevices.removeEventListener('devicechange', refreshDevices)
      for (const url of urls) URL.revokeObjectURL(url)
    }
  })
</script>

<div class="settings-page audio-panel">
  <h1>{t('audio')}</h1>
  <p class="muted">{t('audioHint')}</p>
  <div class="setting-row">
    <label for="audio-input">{t('input')}</label><select
      id="audio-input"
      bind:value={input}
      disabled={recording}
      ><option value="default">{t('systemDefault')}</option
      >{#each devices.filter((d) => d.kind === 'audioinput' && d.deviceId !== 'default') as device, index}<option
          value={device.deviceId}>{device.label || `${t('input')} ${index + 1}`}</option
        >{/each}</select
    >
  </div>
  <div class="setting-row">
    <label for="audio-output">{t('output')}</label><select
      id="audio-output"
      bind:value={output}
      onchange={() => {
        if ('setSinkId' in audio) void audio.setSinkId(output).catch(fail)
      }}
      ><option value="default">{t('systemDefault')}</option
      >{#each devices.filter((d) => d.kind === 'audiooutput' && d.deviceId !== 'default') as device, index}<option
          value={device.deviceId}>{device.label || `${t('output')} ${index + 1}`}</option
        >{/each}</select
    >
  </div>
  <div class="audio-meter">
    <meter min="0" max="1" value={level} aria-label={t('input')}></meter><span
      >{seconds.toFixed(1)} s</span
    >
  </div>
  <div class="settings-buttons">
    <button onclick={record} disabled={recording || busy}>{t('record')}</button><button
      onclick={stop}>{t('stop')}</button
    ><button onclick={tone} disabled={recording}>{t('play')}</button>
  </div>
  <audio controls bind:this={audio}></audio>
  <div class="settings-buttons">
    <button onclick={transcribe} disabled={!recordingUrl || recording || busy || !client.authorized}
      >{t('transcribe')}</button
    ><button
      onclick={speak}
      disabled={!transcript.trim() || recording || busy || !client.authorized}>{t('speak')}</button
    >
  </div>
  <textarea rows="4" bind:value={transcript} aria-label={t('transcribe')}></textarea>
  {#if error}<p class="workbench-error" role="alert">{error}</p>{/if}
</div>
