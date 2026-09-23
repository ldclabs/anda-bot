//! CDN HTTP upload with AES encryption and retry logic.

use crate::cdn::aes_ecb;
use crate::error::{Error, Result};
use crate::types::UPLOAD_MAX_RETRIES;
use crate::util::redact;

/// Build a CDN upload URL from `upload_param` and `filekey`.
pub fn build_cdn_upload_url(cdn_base_url: &str, upload_param: &str, filekey: &str) -> String {
    format!(
        "{cdn_base_url}/upload?encrypted_query_param={}&filekey={}",
        urlencoding::encode(upload_param),
        urlencoding::encode(filekey),
    )
}

/// Build a CDN download URL from `encrypt_query_param`.
pub fn build_cdn_download_url(cdn_base_url: &str, encrypted_query_param: &str) -> String {
    format!(
        "{cdn_base_url}/download?encrypted_query_param={}",
        urlencoding::encode(encrypted_query_param),
    )
}

/// Upload an encrypted buffer to the CDN. Returns the `x-encrypted-param` download parameter.
pub async fn upload_buffer_to_cdn(
    plaintext: &[u8],
    aes_key: &[u8; 16],
    cdn_url: &str,
) -> Result<String> {
    upload_buffer_to_cdn_with_client(&reqwest::Client::new(), plaintext, aes_key, cdn_url).await
}

/// Upload through the caller's shared transport.
pub async fn upload_buffer_to_cdn_with_client(
    client: &reqwest::Client,
    plaintext: &[u8],
    aes_key: &[u8; 16],
    cdn_url: &str,
) -> Result<String> {
    let ciphertext = aes_ecb::encrypt(plaintext, aes_key)?;
    tracing::debug!(
        url = redact::redact_url(cdn_url),
        ciphertext_size = ciphertext.len(),
        "CDN upload"
    );

    let mut last_error: Option<Error> = None;

    for attempt in 1..=UPLOAD_MAX_RETRIES {
        match client
            .post(cdn_url)
            .header("Content-Type", "application/octet-stream")
            .body(ciphertext.clone())
            .send()
            .await
        {
            Ok(res) => {
                let status = res.status().as_u16();
                if (400..500).contains(&status) {
                    let msg = res.text().await.unwrap_or_default();
                    return Err(Error::CdnUpload(format!("client error {status}: {msg}")));
                }
                if status != 200 {
                    let msg = format!("server error {status}");
                    tracing::error!(attempt, status, "CDN server error");
                    last_error = Some(Error::CdnUpload(msg));
                    continue;
                }
                let download_param = res
                    .headers()
                    .get("x-encrypted-param")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                if let Some(param) = download_param {
                    tracing::debug!(attempt, "CDN upload success");
                    return Ok(param);
                }
                last_error = Some(Error::CdnUpload("missing x-encrypted-param header".into()));
            }
            Err(e) => {
                tracing::error!(attempt, error = %e, "CDN upload network error");
                last_error = Some(Error::CdnUpload(e.to_string()));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| Error::CdnUpload(format!("failed after {UPLOAD_MAX_RETRIES} attempts"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_cdn_upload_url_encodes() {
        let url = build_cdn_upload_url("https://cdn.example.com", "param=1&x=2", "key/file");
        assert!(url.starts_with("https://cdn.example.com/upload?"));
        assert!(url.contains("encrypted_query_param=param%3D1%26x%3D2"));
        assert!(url.contains("filekey=key%2Ffile"));
    }

    #[test]
    fn build_cdn_download_url_encodes() {
        let url = build_cdn_download_url("https://cdn.example.com", "enc=val&a=b");
        assert!(url.starts_with("https://cdn.example.com/download?"));
        assert!(url.contains("encrypted_query_param=enc%3Dval%26a%3Db"));
    }
}
