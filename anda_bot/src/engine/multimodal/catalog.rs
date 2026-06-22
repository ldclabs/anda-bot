use anda_core::{Resource, ToolGroupInfo};
use std::path::Path;

pub const IMAGE_UNDERSTANDING_AGENT_NAME: &str = "image_understanding";
pub const AUDIO_UNDERSTANDING_AGENT_NAME: &str = "audio_understanding";
pub const VIDEO_UNDERSTANDING_AGENT_NAME: &str = "video_understanding";
pub const OTHER_UNDERSTANDING_AGENT_NAME: &str = "attachment_understanding";

/// Stable id of the multimodal media-understanding capability group.
pub const MEDIA_UNDERSTANDING_TOOL_GROUP_ID: &str = "media_understanding";

pub const IMAGE_MODEL_LABEL: &str = "image";
pub const AUDIO_MODEL_LABEL: &str = "audio";
pub const VIDEO_MODEL_LABEL: &str = "video";
pub const OTHER_MODEL_LABEL: &str = "flash";

/// Returns the shared [`ToolGroupInfo`] for the media-understanding agents.
///
/// The image, audio, video, and generic attachment understanding agents report
/// this so discovery presents them as one bundle. The registry fills in the
/// member list from the agents actually registered.
pub fn media_understanding_tool_group_info() -> ToolGroupInfo {
    ToolGroupInfo {
        id: MEDIA_UNDERSTANDING_TOOL_GROUP_ID.to_string(),
        title: "Media understanding".to_string(),
        description: "Understand image, audio, video, and document/file attachments or workspace/URL media for downstream text-only reasoning.".to_string(),
        instructions: Some(
            "These agents share one media-understanding workflow. Pick `image_understanding`, `audio_understanding`, or `video_understanding` for matching visual/audio/video inputs, and `attachment_understanding` for PDFs, text files, documents, spreadsheets, slides, logs, or other non-media attachments. Provide `path` or `url` for workspace files and URLs, or pass attached resources directly; use `question` for the caller's focus.".to_string(),
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Other,
}

impl MediaKind {
    pub fn agent_name(self) -> &'static str {
        match self {
            Self::Image => IMAGE_UNDERSTANDING_AGENT_NAME,
            Self::Audio => AUDIO_UNDERSTANDING_AGENT_NAME,
            Self::Video => VIDEO_UNDERSTANDING_AGENT_NAME,
            Self::Other => OTHER_UNDERSTANDING_AGENT_NAME,
        }
    }

    pub fn model_label(self) -> &'static str {
        match self {
            Self::Image => IMAGE_MODEL_LABEL,
            Self::Audio => AUDIO_MODEL_LABEL,
            Self::Video => VIDEO_MODEL_LABEL,
            Self::Other => OTHER_MODEL_LABEL,
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Other => "attachment",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Image => {
                "Understand image attachments, image file paths, or image URLs using the model labeled `image`, returning a textual description for downstream agents."
            }
            Self::Audio => {
                "Understand audio attachments, audio file paths, or audio URLs using the model labeled `audio`, returning transcription and sound notes for downstream agents."
            }
            Self::Video => {
                "Understand video attachments, video file paths, or video URLs using the model labeled `video`, returning a textual summary for downstream agents."
            }
            Self::Other => {
                "Understand non-image/audio/video attachments. Text blobs are handled directly, PDFs are parsed locally with LiteParse, and other formats are delegated to available skills or shell-assisted extraction."
            }
        }
    }

    pub fn tags(self) -> Vec<String> {
        match self {
            Self::Image => ["image"].into_iter().map(ToString::to_string).collect(),
            Self::Audio => ["audio"].into_iter().map(ToString::to_string).collect(),
            Self::Video => ["video"].into_iter().map(ToString::to_string).collect(),
            Self::Other => [
                "text", "txt", "md", "markdown", "pdf", "json", "csv", "tsv", "doc", "docx", "xls",
                "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "html", "xml", "yaml", "toml",
                "log", "document", "file", "other",
            ]
            .into_iter()
            .map(ToString::to_string)
            .collect(),
        }
    }

    pub fn default_question(self) -> &'static str {
        match self {
            Self::Image => {
                "Describe the image for a text-only agent. Include visible objects, people, layout, notable details, and any readable text."
            }
            Self::Audio => {
                "Understand this audio for a text-only agent. Transcribe speech when possible, identify language, speakers if apparent, sound events, tone, and any uncertainty."
            }
            Self::Video => {
                "Understand this video for a text-only agent. Summarize the key visual events, scenes, actions, on-screen text, and audible speech or sounds when available."
            }
            Self::Other => {
                "Understand this attachment for a text-only agent. Extract or summarize useful text, structure, metadata, and uncertainty. Do not invent unavailable content."
            }
        }
    }

    pub fn instructions(self) -> String {
        if self == Self::Other {
            return "You are a specialized attachment understanding subagent. Prefer local, faithful extraction over guessing. For text attachments, preserve the actual content when it is small and summarize it when it is large. For PDFs, use LiteParse extraction results. For other formats, look for a suitable installed skill first, then use safe shell/read-only inspection, and only research a method over the network when local options are insufficient. Return Markdown plain text for the main agent and clearly mark failures or uncertainty.".to_string();
        }

        format!(
            "You are a specialized {kind} understanding subagent. Use the provided {kind} content, file path, or URL only. Answer the caller's question when one is provided; otherwise produce a concise but complete understanding that a text-only main agent can rely on. Return Markdown plain text. Preserve observable facts, transcribe visible or audible text when possible, and clearly mark uncertainty instead of guessing.",
            kind = self.noun()
        )
    }

    pub fn from_resource(resource: &Resource) -> Option<Self> {
        let media_kind = resource
            .mime_type
            .as_deref()
            .and_then(Self::from_mime_type)
            .or_else(|| {
                resource
                    .blob
                    .as_ref()
                    .and_then(|blob| infer2::get(&blob.0))
                    .and_then(|kind| Self::from_mime_type(kind.mime_type()))
            })
            .or_else(|| {
                infer2::get_from_filename(&resource.name)
                    .and_then(|kind| Self::from_mime_type(kind.mime_type()))
            })
            .or_else(|| Self::from_tags(&resource.tags))
            .or_else(|| extension_from_name(&resource.name).and_then(Self::from_extension));

        media_kind.or_else(|| is_other_resource_candidate(resource).then_some(Self::Other))
    }

    pub fn from_mime_type(mime_type: &str) -> Option<Self> {
        let mime_type = mime_type.to_ascii_lowercase();
        if mime_type.starts_with("image/") {
            Some(Self::Image)
        } else if mime_type.starts_with("audio/") {
            Some(Self::Audio)
        } else if mime_type.starts_with("video/") {
            Some(Self::Video)
        } else {
            None
        }
    }

    pub fn from_tags(tags: &[String]) -> Option<Self> {
        for tag in tags {
            let tag = tag.to_ascii_lowercase();
            if tag == "image" {
                return Some(Self::Image);
            }
            if tag == "audio" {
                return Some(Self::Audio);
            }
            if tag == "video" {
                return Some(Self::Video);
            }
        }

        tags.iter()
            .find_map(|tag| Self::from_extension(tag.trim_start_matches('.')))
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "avif" | "bmp" | "gif" | "heic" | "heif" | "jpeg" | "jpg" | "png" | "svg" | "tif"
            | "tiff" | "webp" => Some(Self::Image),
            "aac" | "aiff" | "amr" | "flac" | "m4a" | "mp3" | "mpga" | "oga" | "ogg" | "opus"
            | "pcm" | "wav" => Some(Self::Audio),
            "3gp" | "avi" | "flv" | "m4v" | "mkv" | "mov" | "mp4" | "mpg" | "ogv" | "webm"
            | "wmv" => Some(Self::Video),
            "mpeg" => Some(Self::Audio),
            "pdf" => Some(Self::Other),
            _ => None,
        }
    }
}

fn is_other_resource_candidate(resource: &Resource) -> bool {
    resource.blob.is_some()
        || resource
            .uri
            .as_deref()
            .is_some_and(|uri| !uri.trim().is_empty())
        || !resource.name.trim().is_empty()
        || resource
            .mime_type
            .as_deref()
            .is_some_and(|mime_type| !mime_type.trim().is_empty())
        || resource.size.unwrap_or_default() > 0
        || resource.tags.iter().any(|tag| !tag.trim().is_empty())
}

fn extension_from_name(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|ext| ext.to_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::ByteBufB64;

    #[test]
    fn media_kind_metadata_covers_every_variant() {
        for kind in [
            MediaKind::Image,
            MediaKind::Audio,
            MediaKind::Video,
            MediaKind::Other,
        ] {
            assert!(!kind.agent_name().is_empty());
            assert!(!kind.model_label().is_empty());
            assert!(!kind.noun().is_empty());
            assert!(!kind.description().is_empty());
            assert!(!kind.default_question().is_empty());
            assert!(!kind.instructions().is_empty());
            assert!(!kind.tags().is_empty());
        }
        assert!(MediaKind::Other.instructions().contains("attachment"));
        assert!(MediaKind::Image.instructions().contains("image"));
    }

    #[test]
    fn media_kind_from_mime_tags_and_extension() {
        assert_eq!(
            MediaKind::from_mime_type("IMAGE/PNG"),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_mime_type("audio/mp3"),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            MediaKind::from_mime_type("video/mp4"),
            Some(MediaKind::Video)
        );
        assert_eq!(MediaKind::from_mime_type("application/zip"), None);

        assert_eq!(
            MediaKind::from_tags(&["Image".to_string()]),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_tags(&["audio".to_string()]),
            Some(MediaKind::Audio)
        );
        assert_eq!(
            MediaKind::from_tags(&["video".to_string()]),
            Some(MediaKind::Video)
        );
        assert_eq!(
            MediaKind::from_tags(&[".mp3".to_string()]),
            Some(MediaKind::Audio)
        );
        assert_eq!(MediaKind::from_tags(&["nope".to_string()]), None);

        assert_eq!(MediaKind::from_extension("JPG"), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_extension("flac"), Some(MediaKind::Audio));
        assert_eq!(MediaKind::from_extension("mpeg"), Some(MediaKind::Audio));
        assert_eq!(MediaKind::from_extension("mkv"), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_extension("pdf"), Some(MediaKind::Other));
        assert_eq!(MediaKind::from_extension("unknown"), None);
    }

    #[test]
    fn detects_media_kind_from_mime_before_extension() {
        let resource = Resource {
            name: "clip.mp4".to_string(),
            mime_type: Some("audio/mp4".to_string()),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Audio));
    }

    #[test]
    fn detects_other_kind_for_utf8_blob() {
        let resource = Resource {
            name: "notes.txt".to_string(),
            blob: Some(ByteBufB64(b"hello from a text attachment".to_vec())),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Other));
    }

    #[test]
    fn detects_other_kind_for_pdf() {
        let resource = Resource {
            name: "report.bin".to_string(),
            mime_type: Some("application/pdf".to_string()),
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Other));
    }

    #[test]
    fn detects_media_kind_from_tag_extension() {
        let resource = Resource {
            name: "payload.bin".to_string(),
            tags: vec![".webm".to_string()],
            ..Default::default()
        };

        assert_eq!(MediaKind::from_resource(&resource), Some(MediaKind::Video));
    }

    #[test]
    fn other_resource_candidate_and_group_info_are_complete() {
        assert!(!is_other_resource_candidate(&Resource::default()));
        assert!(is_other_resource_candidate(&Resource {
            name: "x".to_string(),
            ..Default::default()
        }));

        let group = media_understanding_tool_group_info();
        assert_eq!(group.id, MEDIA_UNDERSTANDING_TOOL_GROUP_ID);
        assert_eq!(group.title, "Media understanding");
        assert!(
            group
                .instructions
                .as_deref()
                .is_some_and(|instructions| instructions.contains("attachment_understanding"))
        );
    }
}
