use anda_core::{AgentOutput, BoxError, CompletionRequest, Resource, StateFeatures};
use anda_engine::{
    context::{AgentCtx, CompletionRunner},
    model::{is_retryable_box_error, model_error_retry_after},
};
use std::time::Duration;

pub(crate) const DEFAULT_MODEL_ERROR_RETRY_AFTER: Duration = Duration::from_secs(60);

pub(crate) async fn runner_next_with_retry(
    runner: &mut CompletionRunner,
    operation: &str,
) -> Result<Option<AgentOutput>, BoxError> {
    loop {
        match runner.next().await {
            Ok(output) => return Ok(output),
            Err(err) => {
                let Some(delay) = retry_delay_for_error(&err) else {
                    return Err(err);
                };
                if wait_before_retry(runner, operation, delay, &err).await {
                    continue;
                }
                return runner.next().await;
            }
        }
    }
}

pub(crate) async fn completion_with_retry(
    ctx: &AgentCtx,
    req: CompletionRequest,
    resources: Vec<Resource>,
    operation: &str,
) -> Result<AgentOutput, BoxError> {
    let mut runner = ctx.clone().completion_iter(req, resources);
    let mut last: Option<AgentOutput> = None;

    while let Some(step) = runner_next_with_retry(&mut runner, operation).await? {
        if step.failed_reason.is_some() {
            return Ok(step);
        }
        last = Some(step);
    }

    last.ok_or_else(|| "completion runner returned no output".into())
}

fn retry_delay_for_error(error: &BoxError) -> Option<Duration> {
    if !is_retryable_box_error(error) {
        return None;
    }

    Some(
        model_error_retry_after(error.as_ref() as &(dyn std::error::Error + 'static))
            .unwrap_or(DEFAULT_MODEL_ERROR_RETRY_AFTER),
    )
}

async fn wait_before_retry(
    runner: &CompletionRunner,
    operation: &str,
    delay: Duration,
    error: &BoxError,
) -> bool {
    let retry_after_ms = delay.as_millis() as u64;
    let model = runner.model().model_name();
    log::warn!(
        operation = operation,
        model = model.as_str(),
        retry_after_ms = retry_after_ms;
        "retryable model completion error; retrying after {:?}: {}",
        delay,
        error
    );

    let cancellation = runner.ctx().cancellation_token();
    tokio::select! {
        _ = cancellation.cancelled() => false,
        _ = tokio::time::sleep(delay) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::model::ModelError;

    #[test]
    fn retry_delay_defaults_to_one_minute_for_retryable_model_errors() {
        let err: BoxError = Box::new(ModelError::new("rate limited").with_retryable(true));

        assert_eq!(
            retry_delay_for_error(&err),
            Some(DEFAULT_MODEL_ERROR_RETRY_AFTER)
        );
    }

    #[test]
    fn retry_delay_uses_model_retry_after_when_available() {
        let retry_after = Duration::from_secs(17);
        let err: BoxError = Box::new(
            ModelError::new("rate limited")
                .with_retryable(true)
                .with_retry_after(Some(retry_after)),
        );

        assert_eq!(retry_delay_for_error(&err), Some(retry_after));
    }

    #[test]
    fn retry_delay_ignores_non_retryable_model_errors() {
        let err: BoxError = Box::new(ModelError::new("bad request").with_retryable(false));

        assert_eq!(retry_delay_for_error(&err), None);
    }

    #[tokio::test]
    async fn completion_with_retry_propagates_non_retryable_runner_errors() {
        // The mock engine has no completion model configured, so the first
        // runner step fails with a non-retryable error that must surface
        // directly instead of looping.
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx();
        let result = completion_with_retry(&ctx, CompletionRequest::default(), vec![], "test")
            .await
            .map(|_| ());
        assert!(result.is_err());
    }
}
