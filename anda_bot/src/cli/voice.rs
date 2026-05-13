//! CLI voice input/output helpers.
//!
//! This module records bounded microphone input for `anda voice` and plays audio
//! artifacts returned by the daemon. Wake-word detection is not handled here; a
//! future wake model can decide when to invoke these helpers.

use anda_core::{AgentInput, BoxError, ByteBufB64, Message, RequestMeta, Resource, ToolInput};
use anda_engine::memory::{Conversation, ConversationDelta, ConversationStatus};
use anda_kip::Response as KipResponse;
use clap::Args;
use ic_auth_types::Xid;
use std::{
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::sync::mpsc;

use crate::{
    config,
    engine::{ConversationsTool, ConversationsToolArgs},
    gateway, transcription, tts, util,
};

const AUDIO_EVENT_BUFFER: usize = 16;
const VOICE_POLL_INTERVAL: Duration = Duration::from_millis(1500);
const VOICE_STATUS_INTERVAL: Duration = Duration::from_millis(120);
const VOICE_TTS_CHUNK_CHARS: usize = 800;
const VOICE_TTS_SHORT_CHUNK_CHARS: usize = 80;
const VOICE_TTS_MAX_SHORT_LINES: usize = 4;

#[derive(Args)]
pub struct VoiceCommand {
    /// Agent name. Empty value uses the default agent.
    #[arg(long, default_value = "")]
    name: String,
    /// Recording duration in seconds for each voice turn.
    #[arg(long, default_value_t = 5)]
    record_secs: u64,
    /// Do not play returned speech audio artifacts.
    #[arg(long)]
    no_playback: bool,
    /// Optional request metadata as a JSON object.
    #[arg(long)]
    meta: Option<String>,
}

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
    let initial_conversation_id = base_meta
        .extra
        .get("conversation")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let mut cursor = initialize_voice_cursor(client, initial_conversation_id).await?;
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
            "conversation".to_string(),
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
    let first_status = format!("Synthesizing speech segment 1/{total}");
    let mut current_artifact = match wait_with_voice_status(
        &first_status,
        synthesize_voice_artifact(tts, first_chunk, turn, 0, total),
    )
    .await?
    {
        Some(artifact) => artifact,
        None => return Ok(false),
    };

    for (index, next_chunk) in chunks.iter().enumerate().skip(1) {
        eprintln!(
            "Playing speech segment {}/{}; preparing {}/{}...",
            index,
            total,
            index + 1,
            total
        );
        let playback = voice_channel.play_audio_artifacts(std::slice::from_ref(&current_artifact));
        let synthesis = synthesize_voice_artifact(tts, next_chunk, turn, index, total);
        let (playback_result, synthesis_result) = tokio::join!(playback, synthesis);
        playback_result?;
        current_artifact = synthesis_result?;
    }

    eprintln!("Playing speech segment {total}/{total}...");
    voice_channel
        .play_audio_artifacts(std::slice::from_ref(&current_artifact))
        .await?;
    Ok(true)
}

async fn synthesize_voice_artifact(
    tts: &tts::TtsManager,
    chunk: &str,
    turn: u64,
    index: usize,
    total: usize,
) -> Result<Resource, BoxError> {
    let audio = tts.synthesize(chunk).await?;
    let name = if total == 1 {
        format!("anda_voice_turn_{turn}")
    } else {
        format!("anda_voice_turn_{turn}_part_{}", index + 1)
    };
    Ok(tts.audio_artifact(audio, Some(name)))
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

    let Some((prefix, rest)) = trimmed.split_once(['.', '、', ')']) else {
        return trimmed;
    };
    if !prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit()) {
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
    for ch in segment.chars() {
        if hard_chunk.chars().count() >= max_chars {
            chunks.push(hard_chunk.trim().to_string());
            hard_chunk.clear();
        }
        hard_chunk.push(ch);
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
    conversation_id: u64,
) -> Result<VoiceConversationCursor, BoxError> {
    if conversation_id == 0 {
        return Ok(VoiceConversationCursor::default());
    }

    let conversation = get_conversation(client, conversation_id).await?;
    Ok(VoiceConversationCursor {
        conversation_id: Some(conversation._id),
        seen_messages: conversation.messages.len(),
        seen_artifacts: conversation.artifacts.len(),
    })
}

async fn poll_voice_response(
    client: &gateway::Client,
    cursor: &mut VoiceConversationCursor,
    conversation_id: u64,
) -> Result<String, BoxError> {
    let mut conversation_id = conversation_id;
    reset_voice_cursor_if_needed(cursor, conversation_id);

    loop {
        let delta = get_conversation_delta(
            client,
            conversation_id,
            cursor.seen_messages,
            cursor.seen_artifacts,
        )
        .await?;

        let response_text = assistant_text_from_messages(&delta.messages);
        cursor.seen_messages += delta.messages.len();
        cursor.seen_artifacts += delta.artifacts.len();

        if !response_text.trim().is_empty() {
            return Ok(response_text);
        }

        if let Some(child_id) = delta.child
            && child_id != conversation_id
        {
            conversation_id = child_id;
            reset_voice_cursor_if_needed(cursor, conversation_id);
            continue;
        }

        if is_terminal_conversation_status(&delta.status) {
            if let Some(reason) = delta.failed_reason.as_deref() {
                return Err(format!("voice conversation turn failed: {reason}").into());
            }
            if matches!(delta.status, ConversationStatus::Cancelled) {
                return Err("voice conversation turn was cancelled".into());
            }
            return Ok(response_text);
        }

        tokio::time::sleep(VOICE_POLL_INTERVAL).await;
    }
}

async fn get_conversation(
    client: &gateway::Client,
    conversation_id: u64,
) -> Result<Conversation, BoxError> {
    let output = client
        .tool_call::<ConversationsToolArgs, KipResponse>(&ToolInput::new(
            ConversationsTool::NAME.to_string(),
            ConversationsToolArgs::GetConversation {
                _id: conversation_id,
            },
        ))
        .await?;

    match output.output {
        KipResponse::Ok { result, .. } => Ok(serde_json::from_value::<Conversation>(result)?),
        other => Err(format!("conversation API returned an error: {other:?}").into()),
    }
}

async fn get_conversation_delta(
    client: &gateway::Client,
    conversation_id: u64,
    messages_offset: usize,
    artifacts_offset: usize,
) -> Result<ConversationDelta, BoxError> {
    let output = client
        .tool_call::<ConversationsToolArgs, KipResponse>(&ToolInput::new(
            ConversationsTool::NAME.to_string(),
            ConversationsToolArgs::GetConversationDelta {
                _id: conversation_id,
                messages_offset,
                artifacts_offset,
            },
        ))
        .await?;

    match output.output {
        KipResponse::Ok { result, .. } => Ok(serde_json::from_value::<ConversationDelta>(result)?),
        other => Err(format!("conversation API returned an error: {other:?}").into()),
    }
}

fn reset_voice_cursor_if_needed(cursor: &mut VoiceConversationCursor, conversation_id: u64) {
    if cursor.conversation_id != Some(conversation_id) {
        cursor.conversation_id = Some(conversation_id);
        cursor.seen_messages = 0;
        cursor.seen_artifacts = 0;
    }
}

fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
}

fn assistant_text_from_messages(messages: &[serde_json::Value]) -> String {
    messages
        .iter()
        .filter_map(|raw| serde_json::from_value::<Message>(raw.clone()).ok())
        .filter(|message| message.role == "assistant")
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
        .entry("source".to_string())
        .or_insert(source.into());
    if let Some(workspace) = workspace {
        meta.extra
            .entry("workspace".to_string())
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

        let (audio_tx, mut audio_rx) = mpsc::channel::<AudioInputEvent>(AUDIO_EVENT_BUFFER);
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

        let name = format!("anda_bot_voice_{}.wav", Xid::new());
        let wav_bytes = encode_wav_from_f32(&samples, input.sample_rate, input.channels);
        Ok(audio_resource_from_bytes(
            wav_bytes,
            name,
            Some("Voice input captured by anda CLI".to_string()),
        ))
    }

    /// Play the first-party audio artifacts returned by the agent/TTS pipeline.
    pub async fn play_audio_artifacts(&self, artifacts: &[Resource]) -> Result<(), BoxError> {
        let mut played = false;
        for artifact in artifacts {
            if transcription::is_audio_resource(artifact)
                && let Some(blob) = &artifact.blob
            {
                let path = write_temp_audio_artifact(artifact, &blob.0).await?;
                let play_result = play_audio_file(&path).await;
                let _ = tokio::fs::remove_file(path).await;
                play_result?;
                played = true;
            }
        }

        if !played {
            eprintln!(
                "No playable audio artifact was returned. Check tts.enabled and provider config."
            );
        }
        Ok(())
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
    audio_tx: mpsc::Sender<AudioInputEvent>,
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
    audio_tx: mpsc::Sender<AudioInputEvent>,
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
    audio_tx: mpsc::Sender<AudioInputEvent>,
) -> Result<cpal::Stream, BoxError>
where
    T: cpal::Sample + cpal::SizedSample + Copy + Send + 'static,
    f32: cpal::FromSample<T>,
{
    use cpal::traits::DeviceTrait;

    let data_tx = audio_tx.clone();
    let err_tx = audio_tx;
    let stream = device.build_input_stream(
        stream_config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if data.is_empty() {
                return;
            }

            let samples = data
                .iter()
                .copied()
                .map(|sample| sample.to_sample::<f32>())
                .collect();
            let _ = data_tx.try_send(AudioInputEvent::Samples(samples));
        },
        move |err| {
            log::warn!(name = "channel"; "voice audio stream error: {err}");
            let _ = err_tx.try_send(AudioInputEvent::StreamError(err.to_string()));
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
        let pcm16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
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

async fn play_audio_file(path: &Path) -> Result<(), BoxError> {
    let output = path.to_str().ok_or("invalid temporary playback path")?;
    let mut errors = Vec::new();

    if command_available("ffplay") {
        let mut command = tokio::process::Command::new("ffplay");
        command.args(["-nodisp", "-autoexit", "-loglevel", "quiet", output]);
        match run_process(command, "ffplay").await {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(err.to_string()),
        }
    }

    if cfg!(target_os = "macos") && command_available("afplay") {
        let mut command = tokio::process::Command::new("afplay");
        command.arg(output);
        match run_process(command, "afplay").await {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(err.to_string()),
        }
    }

    if command_available("play") {
        let mut command = tokio::process::Command::new("play");
        command.args(["-q", output]);
        match run_process(command, "play").await {
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

async fn run_process(mut command: tokio::process::Command, label: &str) -> Result<(), BoxError> {
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{label} failed ({}): {}", output.status, stderr.trim()).into())
}

fn command_available(command: &str) -> bool {
    std::process::Command::new("which")
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

        assert_eq!(first, -32767);
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
}
