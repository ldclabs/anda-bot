//! The Brain HTTP/tool contract is KipArgs, not the native KIP envelope.
use anda_core::BoxError;
use anda_engine::memory::{KipArgs, KipOperation};
use anda_kip::Request;

/// Preserve supported fields and refuse native-only options instead of silently
/// dropping execution constraints at the transport boundary.
pub fn http_kip_args(request: Request) -> Result<KipArgs, BoxError> {
    request.validate()?;
    if request.request_id.is_some()
        || request.space.is_some()
        || request.compatibility_profile.is_some()
        || request.ingest.is_some()
        || request.preconditions.is_some()
        || request.context.is_some()
        || request.requires.is_some()
        || request.extensions.is_some()
        || request
            .options
            .as_ref()
            .is_some_and(|options| options.deadline_ms.is_some() || options.extensions.is_some())
    {
        return Err("Brain HTTP does not support native KIP request metadata, preconditions, deadlines or extensions".into());
    }
    Ok(KipArgs {
        command: None,
        operations: Some(
            request
                .operations
                .into_iter()
                .map(|op| KipOperation::Operation(Box::new(op)))
                .collect(),
        ),
        execution: request.execution,
        read: request.read,
        parameters: request.parameters,
        dry_run: request.options.and_then(|options| options.dry_run),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn http_contract_round_trips_supported_fields() {
        let request: Request = serde_json::from_value(json!({
            "kip": "2.0", "operations": [
                {"op_id":"first", "command":"DESCRIBE PRIMER", "parameters":{"x":1}},
                {"op_id":"second", "command":"LIST TYPES LIMIT 3"}
            ], "execution":{"mode":"independent"},
            "read":{"snapshot_token":"opaque-read-coordinate"}, "parameters":{"shared":"value"}, "options":{"dry_run":true}
        }))
        .unwrap();
        let args = http_kip_args(request.clone()).unwrap();
        let body = serde_json::to_value(&args).unwrap();
        assert!(body.get("kip").is_none());
        assert!(body.get("options").is_none());
        assert_eq!(body["dry_run"], true);
        assert_eq!(args.into_request().unwrap(), request);
    }

    #[test]
    fn native_constraints_and_unknown_fields_are_rejected() {
        for extra in [
            json!({"options":{"deadline_ms":10}}),
            json!({"request_id":"r"}),
            json!({"extensions":{"vendor:test":true}}),
        ] {
            let mut request = json!({"kip":"2.0", "operations":[{"command":"DESCRIBE PRIMER"}]});
            request
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(http_kip_args(serde_json::from_value(request).unwrap()).is_err());
        }
        assert!(
            serde_json::from_value::<KipArgs>(json!({"kip":"2.0","command":"DESCRIBE PRIMER"}))
                .is_err()
        );
    }
}
