//! Retry one platform operation, never replay an already delivered prefix.
use anda_core::BoxError;
use std::{fmt, future::Future, time::Duration};

#[derive(Debug)]
pub(crate) struct SendFailure {
    message: String,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
    pub exhausted: bool,
}

impl fmt::Display for SendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SendFailure {}

pub(crate) fn transport_send_error(error: reqwest::Error, message: String) -> BoxError {
    Box::new(SendFailure {
        message,
        retryable: error.is_connect(),
        retry_after: None,
        exhausted: false,
    })
}

pub(crate) fn http_send_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> BoxError {
    let value = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let retry_after = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<f64>().ok())
        .or_else(|| {
            value
                .pointer("/parameters/retry_after")
                .and_then(|v| v.as_f64())
        })
        .or_else(|| value.get("retry_after").and_then(|v| v.as_f64()))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| Duration::from_secs_f64(v.min(3600.0)));
    Box::new(SendFailure {
        message: format!("{status}: {body}"),
        retryable: status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error(),
        retry_after,
        exhausted: false,
    })
}

pub(crate) fn retryable_send_error(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> bool {
    if let Some(error) = error.downcast_ref::<SendFailure>() {
        return error.retryable;
    }
    if let Some(error) = error.downcast_ref::<reqwest::Error>() {
        // A timeout after submission has an unknown delivery outcome.
        return error.is_connect();
    }
    if let Some(error) = error.downcast_ref::<weixin_agent::Error>() {
        return match error {
            weixin_agent::Error::Http(error) => error.is_connect(),
            weixin_agent::Error::Api { errcode, .. } => {
                matches!(errcode, 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        };
    }
    false
}

pub(crate) async fn send_step<T, F, Fut>(
    delivered: &mut usize,
    mut operation: F,
) -> Result<T, BoxError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, BoxError>>,
{
    for attempt in 0..6 {
        match operation().await {
            Ok(value) => {
                *delivered += 1;
                return Ok(value);
            }
            Err(error) => {
                let retryable = retryable_send_error(error.as_ref());
                if !retryable || attempt == 5 {
                    let message = if *delivered > 0 {
                        format!(
                            "{error} ({} parts already delivered; not replaying the message)",
                            *delivered
                        )
                    } else {
                        error.to_string()
                    };
                    return Err(Box::new(SendFailure {
                        message,
                        retryable: retryable && *delivered == 0,
                        retry_after: None,
                        exhausted: true,
                    }));
                }
                let delay = error
                    .downcast_ref::<SendFailure>()
                    .and_then(|e| e.retry_after)
                    .unwrap_or_else(|| Duration::from_millis((500_u64 << attempt).min(5000)));
                tokio::time::sleep(delay).await;
            }
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_exhaustion_does_not_replay_delivered_prefix() {
        let mut delivered = 0;
        send_step(&mut delivered, || async { Ok(()) })
            .await
            .unwrap();
        let mut attempts = 0;
        let error = send_step(&mut delivered, || {
            attempts += 1;
            async {
                Err::<(), _>(http_send_error(
                    reqwest::StatusCode::TOO_MANY_REQUESTS,
                    &Default::default(),
                    r#"{"retry_after":0}"#,
                ))
            }
        })
        .await
        .unwrap_err();
        assert_eq!(attempts, 6);
        assert_eq!(delivered, 1);
        assert!(!retryable_send_error(error.as_ref()));
        assert!(error.downcast_ref::<SendFailure>().unwrap().exhausted);
    }

    #[tokio::test]
    async fn permanent_failure_is_not_retried() {
        let mut calls = 0;
        let error = send_step(&mut 0, || {
            calls += 1;
            async {
                Err::<(), _>(http_send_error(
                    reqwest::StatusCode::FORBIDDEN,
                    &Default::default(),
                    "forbidden",
                ))
            }
        })
        .await
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(!retryable_send_error(error.as_ref()));
    }

    #[test]
    fn retry_after_reads_platform_body_and_http_header() {
        let error = http_send_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            &Default::default(),
            r#"{"parameters":{"retry_after":3}}"#,
        );
        assert_eq!(
            error.downcast_ref::<SendFailure>().unwrap().retry_after,
            Some(Duration::from_secs(3))
        );
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "1.5".parse().unwrap());
        let error = http_send_error(reqwest::StatusCode::TOO_MANY_REQUESTS, &headers, "{}");
        assert_eq!(
            error.downcast_ref::<SendFailure>().unwrap().retry_after,
            Some(Duration::from_millis(1500))
        );
    }
}
