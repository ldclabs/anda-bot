//! File upload orchestration: read → hash → encrypt → getUploadUrl → CDN POST.

use std::fmt::Write;
use std::path::Path;

use base64::Engine;
use rand::Rng;

use crate::api::client::HttpApiClient;
use crate::cdn::aes_ecb;
use crate::cdn::cdn_upload::{build_cdn_upload_url, upload_buffer_to_cdn_with_client};
use crate::error::{Error, Result};
use crate::types::{GetUploadUrlRequest, UploadMediaType};
use crate::util::random::random_hex;

/// Result of a successful CDN upload.
#[derive(Debug, Clone)]
pub struct CdnUploadResult {
    /// CDN download encrypted query parameter.
    pub encrypt_query_param: String,
    /// AES key as base64 string (for `CdnMedia.aes_key`).
    pub aes_key_base64: String,
    /// AES key as hex string.
    pub aes_key_hex: String,
    /// Plaintext file size.
    pub file_size: u64,
    /// Ciphertext file size.
    pub file_size_ciphertext: u64,
    /// File key used for upload.
    pub filekey: String,
}

/// Upload a local file to the Weixin CDN with AES-128-ECB encryption.
pub(crate) async fn upload_file(
    api: &HttpApiClient,
    cdn_base_url: &str,
    file_path: &Path,
    media_type: UploadMediaType,
    to_user_id: &str,
) -> Result<CdnUploadResult> {
    let plaintext = tokio::fs::read(file_path).await?;
    let rawsize = plaintext.len() as u64;
    let rawfilemd5 = {
        use md5::Digest;
        let mut hasher = md5::Md5::new();
        hasher.update(&plaintext);
        format!("{:x}", hasher.finalize())
    };
    let filesize = aes_ecb::padded_size(plaintext.len()) as u64;
    let filekey = generate_filekey();

    // Generate random 16-byte AES key
    let mut aes_key = [0u8; 16];
    rand::rng().fill(&mut aes_key);
    let aes_key_hex = aes_key
        .iter()
        .fold(String::with_capacity(32), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });

    tracing::debug!(
        file = ?file_path,
        rawsize,
        filesize,
        filekey = %filekey,
        "upload_file: starting"
    );

    let upload_resp = api
        .get_upload_url(&GetUploadUrlRequest {
            filekey: filekey.clone(),
            media_type,
            to_user_id: to_user_id.to_owned(),
            rawsize,
            rawfilemd5,
            filesize,
            no_need_thumb: Some(true),
            thumb_rawsize: None,
            thumb_rawfilemd5: None,
            thumb_filesize: None,
            aeskey: aes_key_hex.clone(),
            base_info: api.base_info(),
        })
        .await?;

    let upload_full_url = upload_resp
        .upload_full_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let upload_param = upload_resp.upload_param.as_deref();

    let cdn_url = if let Some(full) = upload_full_url {
        full.to_owned()
    } else if let Some(param) = upload_param {
        build_cdn_upload_url(cdn_base_url, param, &filekey)
    } else {
        return Err(Error::CdnUpload(
            "getUploadUrl returned no upload URL".into(),
        ));
    };

    let download_param =
        upload_buffer_to_cdn_with_client(api.http_client(), &plaintext, &aes_key, &cdn_url).await?;

    let aes_key_base64 = base64::engine::general_purpose::STANDARD.encode(aes_key_hex.as_bytes());

    Ok(CdnUploadResult {
        encrypt_query_param: download_param,
        aes_key_base64,
        aes_key_hex,
        file_size: rawsize,
        file_size_ciphertext: filesize,
        filekey,
    })
}

/// Generate a 32-character hex filekey.
pub fn generate_filekey() -> String {
    random_hex(16)
}
