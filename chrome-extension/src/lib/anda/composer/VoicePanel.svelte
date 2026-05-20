<script lang="ts">
  import type { VoiceProvider } from '$lib/anda/client'
  import { LoaderCircle, Mic, Square } from '@lucide/svelte'

  type VoiceStage = 'idle' | 'recording' | 'processing'

  let {
    voiceStage,
    sending,
    canRecordVoice,
    voiceOrbStyle,
    voiceStatus,
    voiceProvider,
    canUseBrowserSpeech,
    canUseAndaVoice,
    voiceTranscript,
    onToggleRecording,
    onSelectVoiceProvider
  }: {
    voiceStage: VoiceStage
    sending: boolean
    canRecordVoice: boolean
    voiceOrbStyle: string
    voiceStatus: string
    voiceProvider: VoiceProvider
    canUseBrowserSpeech: boolean
    canUseAndaVoice: boolean
    voiceTranscript: string
    onToggleRecording: () => void | Promise<void>
    onSelectVoiceProvider: (provider: VoiceProvider) => void
  } = $props()

  const active = $derived(voiceStage === 'recording' || voiceStage === 'processing' || sending)
</script>

<div
  class="voice-panel relative grid min-h-32 place-items-center overflow-hidden rounded-md border border-emerald-900/10 bg-[#06120f] px-3 py-4 text-white"
  class:active
>
  <div class="voice-field"></div>
  <button
    type="button"
    class="voice-orb relative grid place-items-center"
    class:recording={voiceStage === 'recording'}
    class:processing={voiceStage === 'processing' || sending}
    style={voiceOrbStyle}
    disabled={!canRecordVoice}
    aria-label={voiceStage === 'recording'
      ? chrome.i18n.getMessage('stopRecording')
      : chrome.i18n.getMessage('startRecording')}
    title={voiceStage === 'recording'
      ? chrome.i18n.getMessage('stopRecording')
      : chrome.i18n.getMessage('startRecording')}
    onclick={onToggleRecording}
  >
    <span class="voice-orb-core"></span>
    <span class="voice-orb-icon">
      {#if voiceStage === 'processing' || sending}
        <LoaderCircle class="size-5 animate-spin" />
      {:else if voiceStage === 'recording'}
        <Square class="size-4 fill-current" />
      {:else}
        <Mic class="size-5" />
      {/if}
    </span>
  </button>

  <div class="relative z-10 mt-3 flex items-center gap-2 text-[11px] font-semibold">
    <span class="voice-status-dot" class:recording={voiceStage === 'recording'}></span>
    <span>{voiceStatus}</span>
  </div>
  <div class="voice-service relative z-10 mt-2 flex items-center gap-1 text-[11px]">
    <div class="voice-service-switch" aria-label="Voice service">
      <button
        type="button"
        class:active={voiceProvider === 'chrome'}
        disabled={!canUseBrowserSpeech || voiceStage !== 'idle'}
        title={chrome.i18n.getMessage('useChromeVoice')}
        onclick={() => onSelectVoiceProvider('chrome')}
      >
        Chrome
      </button>
      <button
        type="button"
        class:active={voiceProvider === 'anda'}
        disabled={!canUseAndaVoice || voiceStage !== 'idle'}
        title={chrome.i18n.getMessage('useAndaVoice')}
        onclick={() => onSelectVoiceProvider('anda')}
      >
        Anda
      </button>
    </div>
    <span class="voice-service-label"
      >{voiceProvider === 'chrome'
        ? chrome.i18n.getMessage('chromeVoiceService')
        : chrome.i18n.getMessage('andaVoiceService')}</span
    >
  </div>
  {#if voiceTranscript}
    <div
      class="voice-transcript relative z-10 mt-2 max-w-full truncate px-3 text-center text-[11px] text-emerald-50/90"
    >
      {voiceTranscript}
    </div>
  {/if}
</div>

<style>
  .voice-panel {
    isolation: isolate;
  }

  .voice-panel::before {
    position: absolute;
    inset: -50% -50%;
    content: '';
    background: conic-gradient(
      from 0deg,
      transparent,
      rgba(16, 185, 129, 0.3),
      rgba(59, 130, 246, 0.3),
      rgba(245, 158, 11, 0.3),
      transparent
    );
    filter: blur(40px);
    opacity: 0;
    transition: opacity 500ms ease-in-out;
    z-index: -2;
  }

  .voice-panel.active::before {
    opacity: 1;
    animation: voice-panel-rotate 10s linear infinite;
  }

  .voice-panel::after {
    position: absolute;
    inset: 0;
    content: '';
    background-image: radial-gradient(
      circle at 2px 2px,
      rgba(255, 255, 255, 0.05) 1px,
      transparent 0
    );
    background-size: 24px 24px;
    mask-image: radial-gradient(circle, black 30%, transparent 80%);
    opacity: 0.4;
    z-index: -1;
  }

  .voice-field {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: radial-gradient(circle at center, rgba(16, 185, 129, 0.1) 0%, transparent 70%);
    transform: scale(calc(0.5 + var(--voice-level, 0) * 1.2));
    opacity: calc(0.1 + var(--voice-level, 0) * 0.5);
    transition: transform 150ms cubic-bezier(0.2, 0, 0.3, 1);
  }

  .voice-orb {
    width: 100px;
    height: 100px;
    border: 0;
    border-radius: 999px;
    color: white;
    background: #06120f;
    position: relative;
    display: grid;
    place-items: center;
    transition: transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .voice-orb:hover:not(:disabled) {
    transform: scale(1.05);
  }

  .voice-orb::before {
    content: '';
    position: absolute;
    inset: -2px;
    border-radius: inherit;
    background: linear-gradient(135deg, #10b981, #3b82f6, #f59e0b);
    padding: 2px;
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask-composite: exclude;
    animation: voice-orb-border-rotate 4s linear infinite;
  }

  .voice-orb.recording::after {
    content: '';
    position: absolute;
    inset: -8px;
    border-radius: inherit;
    border: 2px solid rgba(16, 185, 129, 0.4);
    animation: voice-orb-pulse 2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }

  .voice-orb.recording {
    transform: scale(calc(1 + var(--voice-level, 0) * 0.2));
  }

  .voice-orb.processing {
    animation: voice-orb-breathing 2s ease-in-out infinite;
  }

  .voice-orb-core {
    position: absolute;
    inset: 6px;
    border-radius: inherit;
    background: radial-gradient(circle at 30% 30%, rgba(255, 255, 255, 0.1), transparent);
    box-shadow:
      inset 0 0 20px rgba(16, 185, 129, 0.2),
      0 0 30px rgba(16, 185, 129, 0.1);
  }

  .voice-orb-icon {
    position: relative;
    z-index: 2;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.05);
    backdrop-filter: blur(4px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
    transition: all 0.3s ease;
  }

  .voice-orb.recording .voice-orb-icon {
    background: rgba(16, 185, 129, 0.2);
    box-shadow: 0 0 15px rgba(16, 185, 129, 0.4);
  }

  .voice-status-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: #10b981;
    box-shadow: 0 0 10px rgba(16, 185, 129, 0.8);
    transition: all 0.3s ease;
  }

  .voice-status-dot.recording {
    background: #f59e0b;
    box-shadow: 0 0 15px rgba(245, 158, 11, 0.9);
    animation: status-dot-blink 1s ease-in-out infinite;
  }

  @keyframes status-dot-blink {
    50% {
      opacity: 0.5;
      transform: scale(0.8);
    }
  }

  @keyframes voice-orb-pulse {
    0% {
      transform: scale(1);
      opacity: 0.8;
    }
    100% {
      transform: scale(1.5);
      opacity: 0;
    }
  }

  @keyframes voice-orb-breathing {
    0%,
    100% {
      transform: scale(1);
      opacity: 0.9;
    }
    50% {
      transform: scale(1.05);
      opacity: 1;
    }
  }

  @keyframes voice-orb-border-rotate {
    from {
      rotate: 0deg;
    }
    to {
      rotate: 360deg;
    }
  }

  @keyframes voice-panel-rotate {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .voice-service {
    max-width: 100%;
  }

  .voice-service-label {
    max-width: 136px;
    overflow: hidden;
    color: rgba(236, 253, 245, 0.78);
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .voice-service-switch {
    display: inline-flex;
    padding: 2px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 8px;
    background: rgba(6, 18, 15, 0.42);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
  }

  .voice-service-switch button {
    min-width: 48px;
    border: 0;
    border-radius: 6px;
    padding: 3px 8px;
    color: rgba(236, 253, 245, 0.68);
    font-weight: 700;
    line-height: 1.2;
    transition:
      background 140ms ease-out,
      color 140ms ease-out,
      opacity 140ms ease-out;
  }

  .voice-service-switch button.active {
    background: rgba(236, 253, 245, 0.92);
    color: #064e3b;
  }

  .voice-service-switch button:disabled {
    cursor: not-allowed;
  }

  .voice-service-switch button:disabled:not(.active) {
    opacity: 0.42;
  }
</style>
