//! Media message upload and sending, routed by MIME type.

use std::path::Path;
use std::sync::Arc;

use crate::api::client::HttpApiClient;
use crate::cdn::upload::{CdnUploadResult, upload_file};
use crate::error::Result;
use crate::media::mime::get_mime_from_filename;
use crate::messaging::inbound::SendResult;
use crate::messaging::send::generate_client_id;
use crate::types::{
    BaseInfo, CdnMedia, FileItem, ImageItem, MessageItem, MessageItemType, MessageState,
    MessageType, SendMessageRequest, UploadMediaType, VideoItem, WeixinMessage,
};

/// Upload a file and send it as a message, routing by MIME type.
pub(crate) async fn send_media_file(
    api: &Arc<HttpApiClient>,
    cdn_base_url: &str,
    to: &str,
    file_path: &Path,
    text: &str,
    context_token: Option<&str>,
    base_info: BaseInfo,
) -> Result<SendResult> {
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file.bin");
    let mime = get_mime_from_filename(filename);

    let (media_type, build_item): (UploadMediaType, fn(&str, &CdnUploadResult) -> MessageItem) =
        if mime.starts_with("video/") {
            (UploadMediaType::Video, build_video_item)
        } else if mime.starts_with("image/") {
            (UploadMediaType::Image, build_image_item)
        } else {
            (UploadMediaType::File, |fname, u| build_file_item(fname, u))
        };

    let uploaded = upload_file(api, cdn_base_url, file_path, media_type, to).await?;
    let media_item = build_item(filename, &uploaded);

    // Send text and media as separate requests
    if !text.is_empty() {
        let text_req =
            crate::messaging::send::build_text_message(to, text, context_token, base_info.clone());
        api.send_message(&text_req).await?;
    }

    let client_id = generate_client_id();
    let req = SendMessageRequest {
        msg: WeixinMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(to.to_owned()),
            client_id: Some(client_id.clone()),
            message_type: Some(MessageType::Bot),
            message_state: Some(MessageState::Finish),
            item_list: Some(vec![media_item]),
            context_token: context_token.map(String::from),
            ..Default::default()
        },
        base_info,
    };
    api.send_message(&req).await?;

    Ok(SendResult {
        message_id: client_id,
    })
}

fn build_image_item(_filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    #[allow(clippy::cast_possible_wrap)] // file sizes won't exceed i64::MAX
    let mid_size = uploaded.file_size_ciphertext as i64;
    MessageItem {
        item_type: Some(MessageItemType::Image),
        image_item: Some(ImageItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(uploaded.aes_key_hex.as_bytes()),
                ),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            mid_size: Some(mid_size),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_video_item(_filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    #[allow(clippy::cast_possible_wrap)] // file sizes won't exceed i64::MAX
    let video_size = uploaded.file_size_ciphertext as i64;
    MessageItem {
        item_type: Some(MessageItemType::Video),
        video_item: Some(VideoItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(uploaded.aes_key_hex.as_bytes()),
                ),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            video_size: Some(video_size),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_file_item(filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    MessageItem {
        item_type: Some(MessageItemType::File),
        file_item: Some(FileItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(uploaded.aes_key_hex.as_bytes()),
                ),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            file_name: Some(filename.to_owned()),
            len: Some(uploaded.file_size.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
