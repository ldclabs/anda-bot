//! The Bot's existing application-tool envelope, independent of KIP wire versions.
//! Conversation, browser and management tools keep their result/error contract;
//! only the Brain KIP endpoint exchanges KIP 2.0 request/operation envelopes.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ToolResponse {
    /// Successful response containing the request results
    ///
    /// Must be present when the request succeeds.
    Ok {
        /// The application tool defines the result shape.
        result: Json,

        /// Opaque pagination token after the last returned result.
        ///
        /// If present, the caller can pass it back to request the next page.
        #[serde(skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },

    /// Error response containing structured error details
    ///
    /// Must be present when the request fails.
    /// Contains detailed information about what went wrong.
    Err {
        /// Structured error object describing the failure.
        error: ToolError,

        /// Partial result data, if any, when an error occurs.
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Json>,
    },
}

/// Deserialization dispatches on the presence of `error` rather than relying
/// on `#[serde(untagged)]` variant order: an error response that carries a
/// partial `result` (`{"error": ..., "result": ...}`) would otherwise match
/// the `Ok` variant first and silently drop the error.
impl<'de> Deserialize<'de> for ToolResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let mut value = Json::deserialize(deserializer)?;
        let obj = value
            .as_object_mut()
            .ok_or_else(|| D::Error::custom("tool response must be a JSON object"))?;

        // An explicit `"error": null` is the canonical JSON-RPC success shape;
        // treat it as absent rather than as an (unparseable) error payload.
        match obj.remove("error") {
            None | Some(Json::Null) => {}
            Some(error) => {
                let error: ToolError = serde_json::from_value(error)
                    .map_err(|err| D::Error::custom(format!("invalid `error` object: {err}")))?;
                return Ok(ToolResponse::Err {
                    error,
                    result: obj.remove("result"),
                });
            }
        }

        let result = obj
            .remove("result")
            .ok_or_else(|| D::Error::custom("tool response requires `result` or `error`"))?;
        let next_cursor = match obj.remove("next_cursor") {
            None | Some(Json::Null) => None,
            Some(Json::String(cursor)) => Some(cursor),
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "`next_cursor` must be a string, got: {other}"
                )));
            }
        };
        Ok(ToolResponse::Ok {
            result,
            next_cursor,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Json>,
}

#[cfg(test)]
impl ToolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_tool_envelope_and_never_hides_partial_errors() {
        let success = ToolResponse::Ok {
            result: json!([1]),
            next_cursor: Some("next".into()),
        };
        let encoded = serde_json::to_value(&success).unwrap();
        assert_eq!(encoded, json!({"result": [1], "next_cursor": "next"}));
        assert_eq!(
            serde_json::from_value::<ToolResponse>(encoded).unwrap(),
            success
        );
        let partial =
            json!({"error": {"code": "tool_failed", "message": "incomplete"}, "result": [1]});
        assert!(matches!(
            serde_json::from_value::<ToolResponse>(partial).unwrap(),
            ToolResponse::Err {
                result: Some(_),
                ..
            }
        ));
        assert!(serde_json::from_value::<ToolResponse>(json!({})).is_err());
    }
}
