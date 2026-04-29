//! CLI voice input/output helpers.
//!
//! This module records bounded microphone input for `anda voice` and plays audio
//! artifacts returned by the daemon. Wake-word detection is not handled here; a
//! future wake model can decide when to invoke these helpers.

use anda_core::{AgentInput, BoxError, ByteBufB64, Message, RequestMeta, Resource, ToolInput};
use anda_engine::memory::{Conversation, ConversationStatus};
use anda_kip::Response as KipResponse;
use clap::Args;
use ic_auth_types::Xid;
use std::{
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

        eprintln!("Sending voice turn...");
        let output = tokio::select! {
            result = client.agent_run(&input) => result?,
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Voice conversation stopped.");
                break;
            }
        };

        if let Some(reason) = &output.failed_reason {
            eprintln!("Agent failed: {reason}");
        }
        let conversation_id = output
            .conversation
            .ok_or("agent response did not include a conversation id")?;
        let response_text = tokio::select! {
            result = poll_voice_response(client, &mut cursor, conversation_id) => result?,
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Voice conversation stopped.");
                break;
            }
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
            eprintln!("Synthesizing speech...");
            let audio = tokio::select! {
                result = tts.synthesize(response_text.trim()) => result?,
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("Voice conversation stopped.");
                    break;
                }
            };
            let artifact = tts.audio_artifact(audio, Some(format!("anda_voice_turn_{turn}")));
            voice_channel.play_audio_artifacts(&[artifact]).await?;
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
    })
}

async fn poll_voice_response(
    client: &gateway::Client,
    cursor: &mut VoiceConversationCursor,
    conversation_id: u64,
) -> Result<String, BoxError> {
    if cursor.conversation_id != Some(conversation_id) {
        cursor.conversation_id = Some(conversation_id);
        cursor.seen_messages = 0;
    }

    loop {
        let conversation = get_conversation(client, conversation_id).await?;
        let response_text = assistant_text_since(&conversation, cursor.seen_messages);
        let finished = matches!(
            conversation.status,
            ConversationStatus::Completed
                | ConversationStatus::Cancelled
                | ConversationStatus::Failed
        );

        if finished {
            cursor.seen_messages = conversation.messages.len();
            if let Some(reason) = conversation.failed_reason.as_deref() {
                return Err(format!("voice conversation turn failed: {reason}").into());
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

fn assistant_text_since(conversation: &Conversation, offset: usize) -> String {
    conversation
        .messages
        .iter()
        .skip(offset)
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
                play_audio_file(&path).await?;
                let _ = tokio::fs::remove_file(path).await;
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
    if cfg!(target_os = "macos") && command_available("afplay") {
        let mut command = tokio::process::Command::new("afplay");
        command.arg(output);
        return run_process(command, "afplay").await;
    }

    if command_available("ffplay") {
        let mut command = tokio::process::Command::new("ffplay");
        command.args(["-nodisp", "-autoexit", "-loglevel", "quiet", output]);
        return run_process(command, "ffplay").await;
    }

    if command_available("play") {
        let mut command = tokio::process::Command::new("play");
        command.args(["-q", output]);
        return run_process(command, "play").await;
    }

    Err("audio playback requires `afplay`, `ffplay`, or `play` on PATH".into())
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
}
