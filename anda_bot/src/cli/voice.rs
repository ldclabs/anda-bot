//! CLI voice input/output helpers.
//!
//! This module records bounded microphone input for `anda voice` and plays audio
//! artifacts returned by the daemon. Wake-word detection is not handled here; a
//! future wake model can decide when to invoke these helpers.

use anda_core::{AgentInput, BoxError, ByteBufB64, Message, RequestMeta, Resource, ToolInput};
use anda_engine::memory::ConversationStatus;
use ic_auth_types::Xid;
use std::{
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    config, gateway,
    gateway::is_terminal_conversation_status,
    transcription, tts, util,
    util::{request_meta::keys, tool_response::ToolResponse},
};

const VOICE_POLL_INTERVAL: Duration = Duration::from_millis(1500);
const VOICE_STATUS_INTERVAL: Duration = Duration::from_millis(120);
const VOICE_TTS_CHUNK_CHARS: usize = 800;
const VOICE_TTS_SHORT_CHUNK_CHARS: usize = 80;
const VOICE_TTS_MAX_SHORT_LINES: usize = 4;

pub use super::voice_args::VoiceCommand;

struct VoiceRuntime {
    transcription: transcription::TranscriptionManager,
    tts: Option<tts::TtsManager>,
}

#[derive(Debug, Default)]
struct VoiceConversationCursor {
    conversation_id: Option<u64>,
    seen_messages: usize,
    seen_artifacts: usize,
}

pub async fn run_voice_loop(
    client: &gateway::Client,
    cfg: &config::Config,
    cmd: VoiceCommand,
) -> Result<(), BoxError> {
    if cmd.record_secs == 0 {
        return Err("--record-secs must be greater than zero".into());
    }

    let runtime = build_voice_runtime(cfg, !cmd.no_playback)?;
    let voice_channel = VoiceChannel::new();
    let mut base_meta = parse_request_meta(cmd.meta)?.unwrap_or_default();
    add_cli_voice_context(&mut base_meta);
    let mut cursor = initialize_voice_cursor(client, &base_meta).await?;
    let mut turn = 1u64;

    eprintln!("Starting voice conversation. Press Ctrl-C to stop.");
    loop {
        eprintln!("Listening for {}s (turn {turn})...", cmd.record_secs);
        let audio_resource = tokio::select! {
            result = voice_channel.record_microphone_audio(Duration::from_secs(cmd.record_secs)) => result?,
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Voice conversation stopped.");
                break;
            }
        };

        eprintln!("Transcribing voice turn...");
        let prompt = tokio::select! {
            result = transcribe_voice_resource(&runtime.transcription, &audio_resource) => result?,
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Voice conversation stopped.");
                break;
            }
        };
        if prompt.trim().is_empty() {
            eprintln!("No speech was transcribed for this turn.");
            turn += 1;
            continue;
        }
        println!("You: {}", prompt.trim());

        let mut request_meta = base_meta.clone();
        request_meta.extra.insert(
            keys::CONVERSATION.to_string(),
            cursor.conversation_id.unwrap_or_default().into(),
        );

        let mut input = AgentInput::new(cmd.name.clone(), prompt);
        input.meta = Some(request_meta);

        let output =
            match wait_with_voice_status("Sending voice turn", client.agent_run(&input)).await? {
                Some(output) => output,
                None => break,
            };

        if let Some(reason) = &output.failed_reason {
            eprintln!("Agent failed: {reason}");
        }
        let conversation_id = output
            .conversation
            .ok_or("agent response did not include a conversation id")?;
        let response_text = match wait_with_voice_status(
            "Waiting for assistant response",
            poll_voice_response(client, &mut cursor, conversation_id),
        )
        .await?
        {
            Some(response_text) => response_text,
            None => break,
        };
        if response_text.trim().is_empty() {
            eprintln!("No assistant response was found for this turn.");
            turn += 1;
            continue;
        }

        println!("Anda: {}", response_text.trim());
        if !cmd.no_playback {
            let tts = runtime
                .tts
                .as_ref()
                .ok_or("voice playback requires tts.enabled and a configured TTS provider")?;
            if !play_voice_response(tts, &voice_channel, response_text.trim(), turn).await? {
                break;
            }
        }

        turn += 1;
    }

    Ok(())
}

fn build_voice_runtime(cfg: &config::Config, playback: bool) -> Result<VoiceRuntime, BoxError> {
    let http_client =
        util::http_client::build_http_client(cfg.https_proxy.clone(), |client| client)?;
    let transcription =
        transcription::TranscriptionManager::new(&cfg.transcription, http_client.clone())?;
    if !transcription.is_enabled() {
        return Err(
            "anda voice requires transcription.enabled and a configured STT provider".into(),
        );
    }

    let tts = if playback {
        let tts = tts::TtsManager::new(&cfg.tts, http_client)?;
        if !tts.is_enabled() {
            return Err("anda voice playback requires tts.enabled and a configured TTS provider; use --no-playback to disable speech output".into());
        }
        Some(tts)
    } else {
        None
    };

    Ok(VoiceRuntime { transcription, tts })
}

async fn play_voice_response(
    tts: &tts::TtsManager,
    voice_channel: &VoiceChannel,
    text: &str,
    turn: u64,
) -> Result<bool, BoxError> {
    let cancel = CancellationToken::new();
    let operation = play_voice_response_inner(tts, voice_channel, text, turn, &cancel);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => result.map(|()| true),
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
            operation.await?;
            eprintln!("Voice conversation stopped.");
            Ok(false)
        }
    }
}

async fn play_voice_response_inner(
    tts: &tts::TtsManager,
    voice_channel: &VoiceChannel,
    text: &str,
    turn: u64,
    cancel: &CancellationToken,
) -> Result<(), BoxError> {
    let speech_text = prepare_voice_tts_text(text);
    if speech_text.is_empty() {
        return Err("assistant response did not contain speakable text".into());
    }

    let chunks = split_voice_tts_text(&speech_text, VOICE_TTS_CHUNK_CHARS);
    let Some(first_chunk) = chunks.first() else {
        return Err("assistant response did not contain speakable text".into());
    };

    let total = chunks.len();
    eprintln!("Synthesizing speech in {total} segment(s)...");
    let Some(mut current_artifact) =
        synthesize_voice_artifact(tts, first_chunk, turn, 0, total, cancel).await?
    else {
        return Ok(());
    };

    for (index, next_chunk) in chunks.iter().enumerate().skip(1) {
        eprintln!(
            "Playing speech segment {}/{}; preparing {}/{}...",
            index,
            total,
            index + 1,
            total
        );
        let playback =
            voice_channel.play_audio_artifacts(std::slice::from_ref(&current_artifact), cancel);
        let synthesis = synthesize_voice_artifact(tts, next_chunk, turn, index, total, cancel);
        let (playback_result, synthesis_result) = tokio::join!(playback, synthesis);
        playback_result?;
        let Some(next_artifact) = synthesis_result? else {
            return Ok(());
        };
        current_artifact = next_artifact;
    }

    eprintln!("Playing speech segment {total}/{total}...");
    voice_channel
        .play_audio_artifacts(std::slice::from_ref(&current_artifact), cancel)
        .await?;
    Ok(())
}

async fn synthesize_voice_artifact(
    tts: &tts::TtsManager,
    chunk: &str,
    turn: u64,
    index: usize,
    total: usize,
    cancel: &CancellationToken,
) -> Result<Option<Resource>, BoxError> {
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Ok(None),
        result = tts.synthesize(chunk) => result,
    };
    let audio = result.inspect_err(|_| cancel.cancel())?;
    let name = if total == 1 {
        format!("anda_voice_turn_{turn}")
    } else {
        format!("anda_voice_turn_{turn}_part_{}", index + 1)
    };
    Ok(Some(tts.audio_artifact(audio, Some(name))))
}

fn prepare_voice_tts_text(text: &str) -> String {
    text.lines()
        .filter_map(|line| {
            let line = strip_markdown_line_prefix(line);
            let mut normalized = String::with_capacity(line.len());
            let mut previous_was_space = false;
            for ch in line.chars().filter_map(normalize_voice_tts_char) {
                if ch.is_whitespace() {
                    if !previous_was_space {
                        normalized.push(' ');
                    }
                    previous_was_space = true;
                } else {
                    normalized.push(ch);
                    previous_was_space = false;
                }
            }

            let normalized = normalized.trim();
            (!normalized.is_empty()).then(|| normalized.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_markdown_line_prefix(line: &str) -> &str {
    let mut trimmed = line.trim_start();
    while let Some(rest) = trimmed.strip_prefix('>') {
        trimmed = rest.trim_start();
    }
    while let Some(rest) = trimmed.strip_prefix('#') {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }

    let Some((index, marker)) = trimmed
        .char_indices()
        .find(|(_, ch)| matches!(ch, '.' | '、' | ')'))
    else {
        return trimmed;
    };
    let (prefix, suffix) = trimmed.split_at(index);
    let rest = &suffix[marker.len_utf8()..];
    if !prefix.is_empty()
        && prefix.chars().all(|ch| ch.is_ascii_digit())
        && (marker == '、' || rest.starts_with(char::is_whitespace))
    {
        rest.trim_start()
    } else {
        trimmed
    }
}

fn normalize_voice_tts_char(ch: char) -> Option<char> {
    if is_emoji_or_format_control(ch) {
        return None;
    }

    match ch {
        '`' | '*' | '_' | '#' => None,
        '\r' | '\u{00a0}' => Some(' '),
        '—' | '–' => Some('，'),
        _ => Some(ch),
    }
}

fn is_emoji_or_format_control(ch: char) -> bool {
    matches!(
        ch as u32,
        0x200D | 0xFE0E | 0xFE0F
            | 0x2600..=0x27BF
            | 0x1F000..=0x1FAFF
            | 0xE0020..=0xE007F
    )
}

fn split_voice_tts_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current_lines = Vec::new();
    let mut current_chars = 0usize;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let line_chars = line.chars().count();
        if line_chars > max_chars {
            push_voice_tts_lines(&mut chunks, &mut current_lines, &mut current_chars);
            chunks.extend(split_long_voice_tts_line(line, max_chars));
            continue;
        }

        let separator_chars = usize::from(!current_lines.is_empty());
        let next_chars = current_chars + separator_chars + line_chars;
        if !current_lines.is_empty()
            && (next_chars > max_chars
                || current_lines.len() >= VOICE_TTS_MAX_SHORT_LINES
                || current_lines.len() >= 2 && current_chars >= VOICE_TTS_SHORT_CHUNK_CHARS)
        {
            push_voice_tts_lines(&mut chunks, &mut current_lines, &mut current_chars);
        }

        current_chars += usize::from(!current_lines.is_empty()) + line_chars;
        current_lines.push(line.to_string());
    }
    push_voice_tts_lines(&mut chunks, &mut current_lines, &mut current_chars);

    chunks
}

fn push_voice_tts_lines(
    chunks: &mut Vec<String>,
    current_lines: &mut Vec<String>,
    current_chars: &mut usize,
) {
    if !current_lines.is_empty() {
        chunks.push(current_lines.join("\n"));
        current_lines.clear();
        *current_chars = 0;
    }
}

fn split_long_voice_tts_line(line: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for segment in line.split_inclusive(is_tts_sentence_boundary) {
        push_voice_tts_segment(&mut chunks, &mut current, segment, max_chars);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn push_voice_tts_segment(
    chunks: &mut Vec<String>,
    current: &mut String,
    segment: &str,
    max_chars: usize,
) {
    let segment = segment.trim();
    if segment.is_empty() {
        return;
    }

    let current_chars = current.chars().count();
    let segment_chars = segment.chars().count();
    let separator_chars = usize::from(!current.is_empty());
    if current_chars + separator_chars + segment_chars <= max_chars {
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(segment);
        return;
    }

    if !current.is_empty() {
        chunks.push(current.trim().to_string());
        current.clear();
    }

    if segment_chars <= max_chars {
        current.push_str(segment);
        return;
    }

    let mut hard_chunk = String::new();
    let mut hard_chars = 0;
    for ch in segment.chars() {
        if hard_chars >= max_chars {
            chunks.push(hard_chunk.trim().to_string());
            hard_chunk.clear();
            hard_chars = 0;
        }
        hard_chunk.push(ch);
        hard_chars += 1;
    }
    if !hard_chunk.trim().is_empty() {
        current.push_str(hard_chunk.trim());
    }
}

fn is_tts_sentence_boundary(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | '\n')
}

async fn wait_with_voice_status<T>(
    message: &str,
    operation: impl Future<Output = Result<T, BoxError>>,
) -> Result<Option<T>, BoxError> {
    tokio::pin!(operation);
    let mut spinner = VoiceStatusSpinner::new(message);
    let mut interval = tokio::time::interval(VOICE_STATUS_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = &mut operation => {
                spinner.finish();
                return result.map(Some);
            }
            _ = tokio::signal::ctrl_c() => {
                spinner.finish();
                eprintln!("Voice conversation stopped.");
                return Ok(None);
            }
            _ = interval.tick() => spinner.tick(),
        }
    }
}

struct VoiceStatusSpinner<'a> {
    message: &'a str,
    frame: usize,
}

impl<'a> VoiceStatusSpinner<'a> {
    const FRAMES: [&'static str; 4] = ["|", "/", "-", "\\"];

    fn new(message: &'a str) -> Self {
        Self { message, frame: 0 }
    }

    fn tick(&mut self) {
        let frame = Self::FRAMES[self.frame % Self::FRAMES.len()];
        eprint!("\r\x1b[2K{}... {}", self.message, frame);
        let _ = io::stderr().flush();
        self.frame += 1;
    }

    fn finish(&mut self) {
        eprint!("\r\x1b[2K");
        let _ = io::stderr().flush();
    }
}

async fn transcribe_voice_resource(
    transcription: &transcription::TranscriptionManager,
    resource: &Resource,
) -> Result<String, BoxError> {
    let audio = resource
        .blob
        .as_ref()
        .ok_or("voice recording missing inline audio data")?;
    let file_name = transcription::audio_resource_file_name(resource, "voice");
    transcription.transcribe(&audio.0, &file_name).await
}

async fn initialize_voice_cursor(
    client: &gateway::Client,
    meta: &RequestMeta,
) -> Result<VoiceConversationCursor, BoxError> {
    let mut conversation_id = meta
        .get_extra_as::<u64>(keys::CONVERSATION)
        .unwrap_or_default();
    if conversation_id == 0 {
        // Voice uses a stable source just like the TUI. Seed offsets before
        // sending so an existing idle session cannot replay its old answer.
        let mut input = ToolInput::new(
            crate::engine::ConversationsTool::NAME.to_string(),
            crate::engine::ConversationsToolArgs::GetSourceState {},
        );
        input.meta = Some(meta.clone());
        let output = client.tool_call::<_, ToolResponse>(&input).await?;
        let state: crate::engine::SourceState = match output.output {
            ToolResponse::Ok { result, .. } => serde_json::from_value(result)?,
            other => return Err(format!("voice source state unavailable: {other:?}").into()),
        };
        conversation_id = state.conv_id;
    }
    if conversation_id == 0 {
        return Ok(VoiceConversationCursor::default());
    }

    let mut visited = Vec::new();
    loop {
        if visited.contains(&conversation_id) || visited.len() >= gateway::MAX_CONVERSATION_CHAIN {
            return Err("voice conversation child chain contains a cycle or is too long".into());
        }
        visited.push(conversation_id);
        let conversation = client.get_conversation(conversation_id).await?;
        if let Some(child) = conversation.child {
            conversation_id = child;
            continue;
        }
        return Ok(VoiceConversationCursor {
            conversation_id: Some(conversation._id),
            seen_messages: conversation.messages.len(),
            seen_artifacts: conversation.artifacts.len(),
        });
    }
}

async fn poll_voice_response(
    client: &gateway::Client,
    cursor: &mut VoiceConversationCursor,
    conversation_id: u64,
) -> Result<String, BoxError> {
    let mut conversation_id = conversation_id;
    reset_voice_cursor_if_needed(cursor, conversation_id);
    // Same guard as the chat view's poll loop: following `child` skips the
    // poll sleep, so a malformed chain (cycle, or absurd length) would spin
    // into an unbounded sequence of HTTP requests.
    let mut visited: Vec<u64> = vec![conversation_id];
    let mut response_text = String::new();
    let mut received_messages = false;

    loop {
        let delta = client
            .get_conversation_delta(conversation_id, cursor.seen_messages, cursor.seen_artifacts)
            .await?;

        received_messages |= !delta.messages.is_empty();
        let text = assistant_text_from_messages(&delta.messages);
        if !text.trim().is_empty() {
            response_text = text;
        }
        cursor.seen_messages += delta.messages.len();
        cursor.seen_artifacts += delta.artifacts.len();

        if matches!(
            delta.status,
            ConversationStatus::Failed | ConversationStatus::Cancelled
        ) {
            let reason = delta.failed_reason.as_deref().unwrap_or_else(|| {
                if delta.status == ConversationStatus::Cancelled {
                    "conversation cancelled"
                } else {
                    "conversation failed"
                }
            });
            return Err(format!("voice conversation turn failed: {reason}").into());
        }

        if let Some(child_id) = delta.child {
            if visited.contains(&child_id) {
                return Err(
                    format!("conversation child chain contains a cycle at {child_id}").into(),
                );
            }
            if visited.len() >= gateway::MAX_CONVERSATION_CHAIN {
                return Err(format!(
                    "conversation child chain is longer than {}",
                    gateway::MAX_CONVERSATION_CHAIN
                )
                .into());
            }
            visited.push(child_id);
            conversation_id = child_id;
            reset_voice_cursor_if_needed(cursor, conversation_id);
            received_messages = false;
            continue;
        }

        if is_terminal_conversation_status(&delta.status)
            || received_messages && delta.status == ConversationStatus::Idle
        {
            if let Some(reason) = delta.failed_reason.as_deref() {
                return Err(format!("voice conversation turn failed: {reason}").into());
            }
            return Ok(response_text);
        }

        tokio::time::sleep(VOICE_POLL_INTERVAL).await;
    }
}

fn reset_voice_cursor_if_needed(cursor: &mut VoiceConversationCursor, conversation_id: u64) {
    if cursor.conversation_id != Some(conversation_id) {
        cursor.conversation_id = Some(conversation_id);
        cursor.seen_messages = 0;
        cursor.seen_artifacts = 0;
    }
}

fn assistant_text_from_messages(messages: &[serde_json::Value]) -> String {
    messages
        .iter()
        .filter_map(|raw| serde_json::from_value::<Message>(raw.clone()).ok())
        .filter(|message| message.role == "assistant")
        .filter(|message| message.tool_calls().is_empty())
        .filter_map(|message| message.text())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse_request_meta(meta: Option<String>) -> Result<Option<RequestMeta>, BoxError> {
    match meta {
        Some(meta) => Ok(Some(
            serde_json::from_str(&meta).map_err(|e| format!("invalid --meta JSON: {e}"))?,
        )),
        None => Ok(None),
    }
}

fn add_cli_voice_context(meta: &mut RequestMeta) {
    let workspace = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .ok();

    let source = workspace
        .as_ref()
        .map(|dir| format!("cli:voice:{dir}"))
        .unwrap_or_else(|| "cli:voice".to_string());
    meta.extra
        .entry(keys::SOURCE.to_string())
        .or_insert(source.into());
    if let Some(workspace) = workspace {
        meta.extra
            .entry(keys::WORKSPACE.to_string())
            .or_insert(workspace.into());
    }
}

/// Voice input/output helper used by the anda CLI.
#[derive(Debug, Clone, Default)]
pub struct VoiceChannel;

impl VoiceChannel {
    pub fn new() -> Self {
        Self
    }

    /// Record microphone audio for a fixed duration and return it as a WAV resource.
    pub async fn record_microphone_audio(&self, duration: Duration) -> Result<Resource, BoxError> {
        if duration.is_zero() {
            return Err("voice recording duration must be greater than zero".into());
        }

        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<AudioInputEvent>();
        let input = open_default_input_stream(audio_tx)?;

        log::debug!(
            name = "channel";
            "voice channel recording on '{}' ({} Hz, {} channel(s), {:?}) for {:?}",
            input.device_name,
            input.sample_rate,
            input.channels,
            input.sample_format,
            duration,
        );

        let _stream = input.stream;
        let deadline = tokio::time::Instant::now() + duration;
        let expected_samples =
            (duration.as_secs_f64() * f64::from(input.sample_rate) * f64::from(input.channels))
                .ceil() as usize;
        let mut samples = Vec::with_capacity(expected_samples);

        loop {
            let event = tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                event = audio_rx.recv() => event,
            };

            let Some(event) = event else {
                return Err("voice audio stream ended unexpectedly".into());
            };

            match event {
                AudioInputEvent::Samples(chunk) => samples.extend_from_slice(&chunk),
                AudioInputEvent::StreamError(error) => {
                    return Err(format!("voice audio stream error: {error}").into());
                }
            }
        }

        if samples.is_empty() {
            return Err("no audio samples captured from default input device".into());
        }

        drop(_stream);
        let name = format!("anda_bot_voice_{}.wav", Xid::new());
        let wav_bytes = encode_wav_from_f32(&samples, input.sample_rate, input.channels);
        Ok(audio_resource_from_bytes(
            wav_bytes,
            name,
            Some("Voice input captured by anda CLI".to_string()),
        ))
    }

    /// Play the first-party audio artifacts returned by the agent/TTS pipeline.
    async fn play_audio_artifacts(
        &self,
        artifacts: &[Resource],
        cancel: &CancellationToken,
    ) -> Result<(), BoxError> {
        let result = async {
            let mut played = false;
            for artifact in artifacts {
                if cancel.is_cancelled() {
                    return Ok(());
                }
                if transcription::is_audio_resource(artifact)
                    && let Some(blob) = &artifact.blob
                {
                    let path = write_temp_audio_artifact(artifact, &blob.0).await?;
                    let play_result = play_audio_file(&path, cancel).await;
                    let _ = tokio::fs::remove_file(path).await;
                    play_result?;
                    played = true;
                }
            }
            if !played {
                eprintln!("No playable audio artifact was returned. Check tts.enabled and provider config.");
            }
            Ok::<(), BoxError>(())
        }.await;
        result.inspect_err(|_| cancel.cancel())
    }
}

struct AudioInput {
    stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
    sample_format: cpal::SampleFormat,
    device_name: String,
}

enum AudioInputEvent {
    Samples(Vec<f32>),
    StreamError(String),
}

fn open_default_input_stream(
    audio_tx: mpsc::UnboundedSender<AudioInputEvent>,
) -> Result<AudioInput, BoxError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("no default audio input device available")?;
    let device_name = device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| "default input".to_string());
    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let stream_config: cpal::StreamConfig = supported.into();
    let stream = build_input_stream_for_format(&device, &stream_config, sample_format, audio_tx)?;

    stream.play()?;

    Ok(AudioInput {
        stream,
        sample_rate,
        channels,
        sample_format,
        device_name,
    })
}

fn build_input_stream_for_format(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    audio_tx: mpsc::UnboundedSender<AudioInputEvent>,
) -> Result<cpal::Stream, BoxError> {
    match sample_format {
        cpal::SampleFormat::I8 => build_typed_input_stream::<i8>(device, stream_config, audio_tx),
        cpal::SampleFormat::I16 => build_typed_input_stream::<i16>(device, stream_config, audio_tx),
        cpal::SampleFormat::I24 => {
            build_typed_input_stream::<cpal::I24>(device, stream_config, audio_tx)
        }
        cpal::SampleFormat::I32 => build_typed_input_stream::<i32>(device, stream_config, audio_tx),
        cpal::SampleFormat::I64 => build_typed_input_stream::<i64>(device, stream_config, audio_tx),
        cpal::SampleFormat::U8 => build_typed_input_stream::<u8>(device, stream_config, audio_tx),
        cpal::SampleFormat::U16 => build_typed_input_stream::<u16>(device, stream_config, audio_tx),
        cpal::SampleFormat::U24 => {
            build_typed_input_stream::<cpal::U24>(device, stream_config, audio_tx)
        }
        cpal::SampleFormat::U32 => build_typed_input_stream::<u32>(device, stream_config, audio_tx),
        cpal::SampleFormat::U64 => build_typed_input_stream::<u64>(device, stream_config, audio_tx),
        cpal::SampleFormat::F32 => build_typed_input_stream::<f32>(device, stream_config, audio_tx),
        cpal::SampleFormat::F64 => build_typed_input_stream::<f64>(device, stream_config, audio_tx),
        cpal::SampleFormat::DsdU8 | cpal::SampleFormat::DsdU16 | cpal::SampleFormat::DsdU32 => {
            Err(format!("unsupported DSD input sample format: {sample_format:?}").into())
        }
        _ => Err(format!("unsupported input sample format: {sample_format:?}").into()),
    }
}

fn build_typed_input_stream<T>(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    audio_tx: mpsc::UnboundedSender<AudioInputEvent>,
) -> Result<cpal::Stream, BoxError>
where
    T: cpal::Sample + cpal::SizedSample + Copy + Send + 'static,
    f32: cpal::FromSample<T>,
{
    use cpal::traits::DeviceTrait;

    let data_tx = audio_tx.clone();
    let err_tx = audio_tx;
    let stream = device.build_input_stream(
        *stream_config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if data.is_empty() {
                return;
            }

            let samples = data
                .iter()
                .copied()
                .map(|sample| sample.to_sample::<f32>())
                .collect();
            let _ = data_tx.send(AudioInputEvent::Samples(samples));
        },
        move |err| {
            log::warn!(name = "channel"; "voice audio stream error: {err}");
            let _ = err_tx.send(AudioInputEvent::StreamError(err.to_string()));
        },
        None,
    )?;

    Ok(stream)
}

/// Encode raw f32 PCM samples as a minimal 16-bit PCM WAV buffer.
pub fn encode_wav_from_f32(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = u32::from(channels) * sample_rate * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_len = (samples.len() * 2) as u32;
    let file_len = 36 + data_len;

    let mut buf = Vec::with_capacity(file_len as usize + 8);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_len.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    for &sample in samples {
        let sample = sample.clamp(-1.0, 1.0);
        let pcm16 = if sample < 0.0 {
            (sample * 32768.0) as i16
        } else {
            (sample * 32767.0) as i16
        };
        buf.extend_from_slice(&pcm16.to_le_bytes());
    }

    buf
}

fn audio_resource_from_bytes(
    bytes: Vec<u8>,
    name: String,
    description: Option<String>,
) -> Resource {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "wav".to_string());
    let mime_type = audio_mime_for_extension(&extension).unwrap_or("audio/wav");
    let size = bytes.len() as u64;

    Resource {
        tags: vec!["audio".to_string(), extension],
        name,
        description,
        mime_type: Some(mime_type.to_string()),
        blob: Some(ByteBufB64(bytes)),
        size: Some(size),
        ..Default::default()
    }
}

async fn write_temp_audio_artifact(resource: &Resource, bytes: &[u8]) -> Result<PathBuf, BoxError> {
    let extension = resource
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .or_else(|| {
            resource
                .mime_type
                .as_deref()
                .and_then(audio_extension_for_mime)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "mp3".to_string());
    let path = std::env::temp_dir().join(format!("anda_bot_play_{}.{}", Xid::new(), extension));
    tokio::fs::write(&path, bytes).await?;
    Ok(path)
}

async fn play_audio_file(path: &Path, cancel: &CancellationToken) -> Result<(), BoxError> {
    static PLAYERS: OnceLock<Vec<&'static str>> = OnceLock::new();
    let players = PLAYERS.get_or_init(|| {
        ["ffplay", "afplay", "play"]
            .into_iter()
            .filter(|player| {
                (*player != "afplay" || cfg!(target_os = "macos")) && command_available(player)
            })
            .collect()
    });
    let mut errors = Vec::new();
    for &player in players {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let mut command = tokio::process::Command::new(player);
        match player {
            "ffplay" => {
                command.args(["-nodisp", "-autoexit", "-loglevel", "quiet"]);
            }
            "play" => {
                command.arg("-q");
            }
            _ => {}
        }
        command.arg(path);
        match run_process(command, player, cancel).await {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(err.to_string()),
        }
    }
    if errors.is_empty() {
        Err("audio playback requires `ffplay`, `afplay`, or `play` on PATH".into())
    } else {
        Err(format!("audio playback failed: {}", errors.join("; ")).into())
    }
}

async fn run_process(
    mut command: tokio::process::Command,
    label: &str,
    cancel: &CancellationToken,
) -> Result<(), BoxError> {
    let mut child = command.kill_on_drop(true).stdin(Stdio::null()).spawn()?;
    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            child.kill().await?;
            return Ok(());
        }
        status = child.wait() => status?,
    };
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed ({status})").into())
    }
}

fn command_available(command: &str) -> bool {
    let checker = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    std::process::Command::new(checker)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn audio_mime_for_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "flac" => Some("audio/flac"),
        "mp3" | "mpeg" | "mpga" => Some("audio/mpeg"),
        "mp4" | "m4a" => Some("audio/mp4"),
        "oga" | "ogg" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "pcm" => Some("audio/pcm"),
        "wav" => Some("audio/wav"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

fn audio_extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type.to_ascii_lowercase().as_str() {
        "audio/flac" => Some("flac"),
        "audio/mp4" | "audio/x-m4a" => Some("m4a"),
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/ogg" | "audio/oga" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/webm" => Some("webm"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_valid() {
        let samples = vec![0.0f32; 100];
        let wav = encode_wav_from_f32(&samples, 16000, 1);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16000);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 200);
    }

    #[test]
    fn wav_encodes_clipped_samples() {
        let samples = vec![-2.0f32, 2.0, 0.0];
        let wav = encode_wav_from_f32(&samples, 16000, 1);

        let first = i16::from_le_bytes(wav[44..46].try_into().unwrap());
        let second = i16::from_le_bytes(wav[46..48].try_into().unwrap());
        let third = i16::from_le_bytes(wav[48..50].try_into().unwrap());

        assert_eq!(first, -32768);
        assert_eq!(second, 32767);
        assert_eq!(third, 0);
    }

    #[test]
    fn voice_resource_has_audio_metadata() {
        let resource = audio_resource_from_bytes(vec![1, 2, 3], "sample.wav".to_string(), None);

        assert_eq!(resource.name, "sample.wav");
        assert_eq!(resource.tags, vec!["audio", "wav"]);
        assert_eq!(resource.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(resource.size, Some(3));
    }

    #[test]
    fn assistant_text_from_messages_uses_assistant_delta_only() {
        let messages = vec![
            serde_json::json!({
                "role": "user",
                "content": [{"type": "Text", "text": "hello"}],
            }),
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "Text", "text": "hi there"}],
            }),
        ];

        assert_eq!(assistant_text_from_messages(&messages), "hi there");
    }

    #[test]
    fn prepare_voice_tts_text_removes_markdown_and_emoji() {
        let text = "## 回应 ✨\n- 确实，被中断了 😅\n1. 我们继续测试。";

        assert_eq!(
            prepare_voice_tts_text(text),
            "回应\n确实，被中断了\n我们继续测试。"
        );
    }

    #[test]
    fn split_voice_tts_text_keeps_chunks_under_limit() {
        let chunks = split_voice_tts_text("第一句。第二句很长。第三句。", 8);

        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 8));
        assert_eq!(chunks, vec!["第一句。", "第二句很长。", "第三句。"]);
    }

    #[test]
    fn split_voice_tts_text_groups_short_lines() {
        let chunks = split_voice_tts_text("一。\n二。\n三。\n四。\n五。", VOICE_TTS_CHUNK_CHARS);

        assert_eq!(chunks, vec!["一。\n二。\n三。\n四。", "五。"]);
    }

    #[test]
    fn split_voice_tts_text_prefers_two_lines_once_substantial() {
        let line =
            "这是一行足够长的语音合成分段测试内容，用来触发两行一段的策略，并保持单行不超过限制。";
        let chunks =
            split_voice_tts_text(&format!("{line}\n{line}\n{line}"), VOICE_TTS_CHUNK_CHARS);

        assert_eq!(chunks, vec![format!("{line}\n{line}"), line.to_string()]);
    }

    #[test]
    fn prepare_voice_tts_text_strips_markdown_and_emoji() {
        let input = "# Heading\n> quote\n- bullet item\n1. numbered\n`code` **bold** 🎉\n\n   spaced   out   ";
        let out = prepare_voice_tts_text(input);
        assert!(out.contains("Heading"));
        assert!(out.contains("quote"));
        assert!(out.contains("bullet item"));
        assert!(out.contains("numbered"));
        assert!(!out.contains('#'));
        assert!(!out.contains('`'));
        assert!(!out.contains('*'));
        assert!(!out.contains('🎉'));
        assert!(out.contains("spaced out"));
    }

    #[test]
    fn strip_markdown_line_prefix_handles_list_and_numbered_markers() {
        assert_eq!(strip_markdown_line_prefix("  > # quoted"), "quoted");
        assert_eq!(strip_markdown_line_prefix("- item"), "item");
        assert_eq!(strip_markdown_line_prefix("* item"), "item");
        assert_eq!(strip_markdown_line_prefix("12. step"), "step");
        assert_eq!(strip_markdown_line_prefix("3、列表"), "列表");
        // Non-numeric prefix before a separator is left intact.
        assert_eq!(strip_markdown_line_prefix("一、列表"), "一、列表");
        assert_eq!(strip_markdown_line_prefix("v1.0 release"), "v1.0 release");
        for text in ["3.14 is pi", "2026.09.23", "1.2.3", "12)items"] {
            assert_eq!(prepare_voice_tts_text(text), text);
        }
        assert_eq!(strip_markdown_line_prefix("12) step"), "step");
    }

    #[test]
    fn normalize_voice_tts_char_rewrites_punctuation() {
        assert_eq!(normalize_voice_tts_char('`'), None);
        assert_eq!(normalize_voice_tts_char('#'), None);
        assert_eq!(normalize_voice_tts_char('\u{00a0}'), Some(' '));
        assert_eq!(normalize_voice_tts_char('—'), Some('，'));
        assert_eq!(normalize_voice_tts_char('a'), Some('a'));
        assert!(normalize_voice_tts_char('🎉').is_none());
    }

    #[test]
    fn emoji_and_sentence_boundary_predicates() {
        assert!(is_emoji_or_format_control('🎉'));
        assert!(is_emoji_or_format_control('\u{200D}'));
        assert!(!is_emoji_or_format_control('a'));

        assert!(is_tts_sentence_boundary('。'));
        assert!(is_tts_sentence_boundary('?'));
        assert!(!is_tts_sentence_boundary('x'));
    }

    #[test]
    fn split_long_voice_tts_line_breaks_on_sentences_and_hard_limit() {
        let line = "First sentence. Second sentence! Third?";
        let chunks = split_long_voice_tts_line(line, 16);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 16));
        assert!(!chunks.is_empty());

        // A single oversized token is hard-split.
        let hard = split_long_voice_tts_line(&"x".repeat(50), 10);
        assert!(hard.iter().all(|chunk| chunk.chars().count() <= 10));
    }

    #[test]
    fn split_voice_tts_text_returns_empty_for_zero_max() {
        assert!(split_voice_tts_text("anything", 0).is_empty());
    }

    #[test]
    fn reset_voice_cursor_clears_counters_on_new_conversation() {
        let mut cursor = VoiceConversationCursor {
            conversation_id: Some(1),
            seen_messages: 5,
            seen_artifacts: 2,
        };
        reset_voice_cursor_if_needed(&mut cursor, 1);
        assert_eq!(cursor.seen_messages, 5);

        reset_voice_cursor_if_needed(&mut cursor, 2);
        assert_eq!(cursor.conversation_id, Some(2));
        assert_eq!(cursor.seen_messages, 0);
        assert_eq!(cursor.seen_artifacts, 0);
    }

    #[test]
    fn assistant_text_from_messages_joins_assistant_replies() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": [{"type": "Text", "text": "hi"}]}),
            serde_json::json!({"role": "assistant", "content": [{"type": "Text", "text": "hello"}]}),
            serde_json::json!({"role": "assistant", "content": [{"type": "Text", "text": "again"}]}),
        ];
        let text = assistant_text_from_messages(&messages);
        assert_eq!(text, "hello\n\nagain");
    }

    #[test]
    fn parse_request_meta_handles_valid_invalid_and_none() {
        assert!(parse_request_meta(None).unwrap().is_none());
        assert!(
            parse_request_meta(Some("{\"user\":\"alice\"}".to_string()))
                .unwrap()
                .is_some()
        );
        assert!(parse_request_meta(Some("not json".to_string())).is_err());
    }

    #[test]
    fn add_cli_voice_context_sets_source_and_workspace() {
        let mut meta = RequestMeta::default();
        add_cli_voice_context(&mut meta);
        assert!(meta.extra.contains_key("source"));

        // Existing source is preserved.
        let mut preset = RequestMeta::default();
        preset.extra.insert("source".to_string(), "preset".into());
        add_cli_voice_context(&mut preset);
        assert_eq!(
            preset.extra.get("source").and_then(|v| v.as_str()),
            Some("preset")
        );
    }

    #[test]
    fn voice_status_spinner_ticks_and_finishes() {
        let mut spinner = VoiceStatusSpinner::new("working");
        spinner.tick();
        spinner.tick();
        spinner.finish();
        // Finishing twice is safe.
        spinner.finish();
    }

    #[test]
    fn voice_channel_constructs() {
        let _ = VoiceChannel::new();
        let _ = VoiceChannel;
    }

    use anda_core::ByteBufB64;
    use anda_engine::memory::ConversationDelta;
    use axum::{Router, extract::State, routing};
    use std::{collections::HashMap, sync::Arc};

    async fn voice_gateway_handler(
        State(state): State<Arc<HashMap<u64, ConversationDelta>>>,
        axum::Json(request): axum::Json<anda_core::http::RPCRequest>,
    ) -> axum::Json<serde_json::Value> {
        let (input,): (anda_core::ToolInput<serde_json::Value>,) =
            serde_json::from_slice(&request.params).unwrap();
        let id = input.args["_id"].as_u64().unwrap_or_default();
        let delta = state.get(&id).expect("known conversation");
        let response = crate::util::tool_response::ToolResponse::Ok {
            result: serde_json::to_value(delta).unwrap(),
            next_cursor: None,
        };
        let output: anda_core::ToolOutput<crate::util::tool_response::ToolResponse> =
            anda_core::ToolOutput::new(response);
        let rpc: anda_core::http::RPCResponse =
            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()));
        axum::Json(serde_json::to_value(&rpc).unwrap())
    }

    async fn spawn_voice_gateway(deltas: HashMap<u64, ConversationDelta>) -> gateway::Client {
        let app = Router::new()
            .route("/engine/default", routing::post(voice_gateway_handler))
            .with_state(Arc::new(deltas));
        let base_url = crate::test_support::spawn_http_mock(app).await;
        gateway::Client::new(base_url, "token".to_string())
    }

    fn working_delta(id: u64, child: Option<u64>) -> ConversationDelta {
        ConversationDelta {
            _id: id,
            messages: Vec::new(),
            artifacts: Vec::new(),
            status: ConversationStatus::Working,
            usage: Default::default(),
            failed_reason: None,
            updated_at: 0,
            child,
        }
    }

    fn assistant_message(text: &str) -> serde_json::Value {
        serde_json::json!({"role":"assistant", "content":[{"type":"Text", "text":text}]})
    }

    #[tokio::test]
    async fn voice_waits_past_stale_idle_and_intermediate_text_until_turn_is_idle() {
        let mut stale = working_delta(1, None);
        stale.status = ConversationStatus::Idle;
        let mut intermediate = working_delta(1, None);
        intermediate.messages = vec![assistant_message("Checking the files...")];
        let mut final_answer = working_delta(1, None);
        final_answer.messages = vec![assistant_message("The answer is 42.")];
        let mut idle = working_delta(1, None);
        idle.status = ConversationStatus::Idle;
        let script = Arc::new(parking_lot::Mutex::new(std::collections::VecDeque::from([
            (5, stale),
            (5, intermediate),
            (6, final_answer),
            (7, idle),
        ])));
        let observed = script.clone();
        let app = Router::new().route(
            "/engine/default",
            routing::post(
                move |axum::Json(request): axum::Json<anda_core::http::RPCRequest>| {
                    let script = observed.clone();
                    async move {
                        let (input,): (ToolInput<serde_json::Value>,) =
                            serde_json::from_slice(&request.params).unwrap();
                        let (offset, delta) = script.lock().pop_front().expect("expected poll");
                        assert_eq!(input.args["messages_offset"], offset);
                        let output = anda_core::ToolOutput::new(ToolResponse::Ok {
                            result: serde_json::to_value(delta).unwrap(),
                            next_cursor: None,
                        });
                        let rpc: anda_core::http::RPCResponse =
                            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()));
                        axum::Json(serde_json::to_value(rpc).unwrap())
                    }
                },
            ),
        );
        let client = gateway::Client::new(
            crate::test_support::spawn_http_mock(app).await,
            "token".into(),
        );
        let mut cursor = VoiceConversationCursor {
            conversation_id: Some(1),
            seen_messages: 5,
            seen_artifacts: 0,
        };
        let answer = poll_voice_response(&client, &mut cursor, 1).await.unwrap();
        assert_eq!(answer, "The answer is 42.");
        assert!(script.lock().is_empty());
        assert_eq!(cursor.seen_messages, 7);
    }

    #[tokio::test]
    async fn voice_follows_child_before_returning_text_and_surfaces_failure() {
        let mut parent = working_delta(1, Some(2));
        parent.status = ConversationStatus::Completed;
        parent.messages = vec![assistant_message("Before compaction")];
        let mut child = working_delta(2, None);
        child.status = ConversationStatus::Idle;
        child.messages = vec![assistant_message("Final answer")];
        let client = spawn_voice_gateway(HashMap::from([(1, parent), (2, child)])).await;
        let mut cursor = VoiceConversationCursor::default();
        assert_eq!(
            poll_voice_response(&client, &mut cursor, 1).await.unwrap(),
            "Final answer"
        );
        assert_eq!(cursor.conversation_id, Some(2));

        for status in [ConversationStatus::Failed, ConversationStatus::Cancelled] {
            let mut delta = working_delta(3, None);
            delta.status = status;
            delta.messages = vec![assistant_message("Partial output")];
            let client = spawn_voice_gateway(HashMap::from([(3, delta)])).await;
            assert!(
                poll_voice_response(&client, &mut VoiceConversationCursor::default(), 3)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn voice_initializes_offsets_from_source_and_latest_child() {
        let app = Router::new().route(
            "/engine/default",
            routing::post(
                |axum::Json(request): axum::Json<anda_core::http::RPCRequest>| async move {
                    let (input,): (ToolInput<serde_json::Value>,) =
                        serde_json::from_slice(&request.params).unwrap();
                    let result = match input.args["type"].as_str().unwrap() {
                        "GetSourceState" => {
                            assert_eq!(
                                input
                                    .meta
                                    .unwrap()
                                    .get_extra_as::<String>("source")
                                    .as_deref(),
                                Some("cli:voice:test")
                            );
                            serde_json::json!({"c":1})
                        }
                        "GetConversation" => {
                            let id = input.args["_id"].as_u64().unwrap();
                            serde_json::to_value(anda_engine::memory::Conversation {
                                _id: id,
                                child: (id == 1).then_some(2),
                                messages: vec![assistant_message("Old answer")],
                                ..Default::default()
                            })
                            .unwrap()
                        }
                        _ => panic!("unexpected request"),
                    };
                    let output = anda_core::ToolOutput::new(ToolResponse::Ok {
                        result,
                        next_cursor: None,
                    });
                    let rpc: anda_core::http::RPCResponse =
                        Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()));
                    axum::Json(serde_json::to_value(rpc).unwrap())
                },
            ),
        );
        let client = gateway::Client::new(
            crate::test_support::spawn_http_mock(app).await,
            "token".into(),
        );
        let meta = serde_json::from_value(serde_json::json!({"source":"cli:voice:test"})).unwrap();
        let cursor = initialize_voice_cursor(&client, &meta).await.unwrap();
        assert_eq!(cursor.conversation_id, Some(2));
        assert_eq!(cursor.seen_messages, 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn playback_cancellation_reaps_the_child() {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("player.pid");
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args(["-c", "echo $$ > \"$1\"; exec sleep 30", "player"])
            .arg(&pid_path);
        let cancel = CancellationToken::new();
        let operation = run_process(command, "test player", &cancel);
        let stop = async {
            let pid: i32 = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Ok(text) = tokio::fs::read_to_string(&pid_path).await
                        && let Ok(pid) = text.trim().parse()
                    {
                        break pid;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            cancel.cancel();
            pid
        };
        let (result, pid) = tokio::time::timeout(Duration::from_secs(6), async {
            tokio::join!(operation, stop)
        })
        .await
        .unwrap();
        result.unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "player must be reaped");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[tokio::test]
    async fn synthesis_cancellation_interrupts_first_and_prefetched_segments() {
        let started = Arc::new(tokio::sync::Notify::new());
        let observed = started.clone();
        let app = Router::new().route(
            "/speech",
            routing::post(move || {
                let started = observed.clone();
                async move {
                    started.notify_one();
                    std::future::pending::<&'static str>().await
                }
            }),
        );
        let url = crate::test_support::spawn_http_mock(app).await;
        let tts = tts::TtsManager::new(
            &config::TtsConfig {
                enabled: true,
                default_provider: "stepfun".into(),
                stepfun: Some(config::StepFunTtsConfig {
                    api_key: "test".into(),
                    api_url: format!("{url}/speech"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            crate::util::http_client::new_reqwest_client(),
        )
        .unwrap();
        for index in [0, 1] {
            let cancel = CancellationToken::new();
            let stop = async {
                started.notified().await;
                cancel.cancel();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(
                    synthesize_voice_artifact(&tts, "hello", 1, index, 2, &cancel),
                    stop
                )
            })
            .await
            .unwrap();
            assert!(result.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn poll_voice_response_stops_on_child_chain_cycle() {
        // Following `child` skips the poll sleep, so without the guard a
        // cyclic chain would spin this loop on HTTP requests forever.
        let client = spawn_voice_gateway(HashMap::from([
            (1, working_delta(1, Some(2))),
            (2, working_delta(2, Some(1))),
        ]))
        .await;
        let mut cursor = VoiceConversationCursor::default();

        let err = poll_voice_response(&client, &mut cursor, 1)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cycle"), "{err}");
    }
}
