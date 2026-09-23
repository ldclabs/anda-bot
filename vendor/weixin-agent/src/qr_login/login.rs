//! QR code login API — only HTTP calls, no credential persistence.

use std::fmt::Write as _;
use std::time::Duration;

use crate::api::client::HttpApiClient;
use crate::error::Result;
use crate::types::{
    DEFAULT_ILINK_BOT_TYPE, DEFAULT_QR_POLL_TIMEOUT_MS, QrCodeResponse, QrStatusResponse,
};

/// QR login session returned by [`QrLoginApi::start`].
#[derive(Debug, Clone)]
pub struct QrLoginSession {
    /// QR code token string.
    pub qrcode: String,
    /// QR code image URL.
    pub qrcode_img_content: String,
}

/// Login status returned by [`QrLoginApi::poll_status`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LoginStatus {
    /// Waiting for scan.
    Wait,
    /// QR code scanned, awaiting confirmation.
    Scanned,
    /// Scanned but needs IDC redirect.
    ScannedButRedirect {
        /// New host to redirect polling to.
        redirect_host: String,
    },
    /// Login confirmed.
    Confirmed {
        /// Bot authentication token.
        bot_token: String,
        /// Bot ID.
        ilink_bot_id: String,
        /// API base URL.
        base_url: String,
        /// User ID of the person who scanned.
        ilink_user_id: String,
    },
    /// QR code expired.
    Expired,
    /// Server requires a verification code (pair-code displayed on phone).
    NeedVerifyCode,
    /// Too many wrong verification codes; QR code must be refreshed.
    VerifyCodeBlocked,
    /// Bot is already bound to this instance; no new credentials issued.
    BindedRedirect,
}

/// QR login API wrapper.
pub struct QrLoginApi<'a> {
    api: &'a HttpApiClient,
}

impl<'a> QrLoginApi<'a> {
    /// Create a new QR login API handle.
    pub(crate) fn new(api: &'a HttpApiClient) -> Self {
        Self { api }
    }

    /// Fetch a new QR code via POST. `bot_type` defaults to `"3"`.
    /// `local_tokens` sends up to 10 existing bot tokens to the server.
    pub async fn start(
        &self,
        bot_type: Option<&str>,
        local_tokens: &[String],
    ) -> Result<QrLoginSession> {
        let bt = bot_type.unwrap_or(DEFAULT_ILINK_BOT_TYPE);
        let endpoint = format!(
            "ilink/bot/get_bot_qrcode?bot_type={}",
            urlencoding::encode(bt)
        );
        let tokens: Vec<&str> = local_tokens.iter().take(10).map(String::as_str).collect();
        let body = serde_json::json!({ "local_token_list": tokens });
        let raw = self.api.api_post(&endpoint, &body, None).await?;
        let resp: QrCodeResponse = serde_json::from_str(&raw)?;
        Ok(QrLoginSession {
            qrcode: resp.qrcode,
            qrcode_img_content: resp.qrcode_img_content,
        })
    }

    /// Poll the login status for a QR session.
    /// Pass `verify_code` when the server returned [`LoginStatus::NeedVerifyCode`].
    pub async fn poll_status(
        &self,
        session: &QrLoginSession,
        verify_code: Option<&str>,
    ) -> Result<LoginStatus> {
        let mut endpoint = format!(
            "ilink/bot/get_qrcode_status?qrcode={}",
            urlencoding::encode(&session.qrcode)
        );
        if let Some(code) = verify_code {
            let _ = write!(endpoint, "&verify_code={}", urlencoding::encode(code));
        }
        let raw = match self
            .api
            .api_get(&endpoint, Duration::from_millis(DEFAULT_QR_POLL_TIMEOUT_MS))
            .await
        {
            Ok(r) => r,
            Err(crate::error::Error::Http(e)) if e.is_timeout() => {
                return Ok(LoginStatus::Wait);
            }
            Err(e) => return Err(e),
        };

        let resp: QrStatusResponse = serde_json::from_str(&raw)?;
        Ok(match resp.status.as_str() {
            "scaned" => LoginStatus::Scanned,
            "scaned_but_redirect" => LoginStatus::ScannedButRedirect {
                redirect_host: resp.redirect_host.unwrap_or_default(),
            },
            "confirmed" => LoginStatus::Confirmed {
                bot_token: resp.bot_token.unwrap_or_default(),
                ilink_bot_id: resp.ilink_bot_id.unwrap_or_default(),
                base_url: resp.baseurl.unwrap_or_default(),
                ilink_user_id: resp.ilink_user_id.unwrap_or_default(),
            },
            "expired" => LoginStatus::Expired,
            "need_verifycode" => LoginStatus::NeedVerifyCode,
            "verify_code_blocked" => LoginStatus::VerifyCodeBlocked,
            "binded_redirect" => LoginStatus::BindedRedirect,
            _ => LoginStatus::Wait,
        })
    }
}

/// Standalone QR login API that owns its HTTP client.
/// Use this when you need QR login before creating a full [`crate::WeixinClient`].
pub struct StandaloneQrLogin {
    api: HttpApiClient,
}

impl StandaloneQrLogin {
    /// Create from a [`crate::WeixinConfig`].
    pub fn new(config: &crate::config::WeixinConfig) -> Self {
        Self {
            api: HttpApiClient::new(config),
        }
    }

    /// Fetch a new QR code.
    pub async fn start(
        &self,
        bot_type: Option<&str>,
        local_tokens: &[String],
    ) -> Result<QrLoginSession> {
        QrLoginApi::new(&self.api)
            .start(bot_type, local_tokens)
            .await
    }

    /// Poll the login status.
    pub async fn poll_status(
        &self,
        session: &QrLoginSession,
        verify_code: Option<&str>,
    ) -> Result<LoginStatus> {
        QrLoginApi::new(&self.api)
            .poll_status(session, verify_code)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_need_verify_code_status() {
        let json = r#"{"status":"need_verifycode"}"#;
        let resp: QrStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "need_verifycode");
    }

    #[test]
    fn parse_verify_code_blocked_status() {
        let json = r#"{"status":"verify_code_blocked"}"#;
        let resp: QrStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "verify_code_blocked");
    }

    #[test]
    fn parse_binded_redirect_status() {
        let json = r#"{"status":"binded_redirect"}"#;
        let resp: QrStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "binded_redirect");
    }

    #[test]
    fn verify_code_appended_to_endpoint() {
        let qrcode = "test_qr";
        let mut endpoint = format!(
            "ilink/bot/get_qrcode_status?qrcode={}",
            urlencoding::encode(qrcode)
        );
        let verify_code = Some("1234");
        if let Some(code) = verify_code {
            let _ = write!(endpoint, "&verify_code={}", urlencoding::encode(code));
        }
        assert!(endpoint.contains("verify_code=1234"));
        assert!(endpoint.contains("qrcode=test_qr"));
    }
}
