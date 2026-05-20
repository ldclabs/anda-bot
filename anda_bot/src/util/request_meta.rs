use anda_core::RequestMeta;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub fn request_meta_extra_as<T>(meta: &RequestMeta, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    extra_map_as(&meta.extra, key)
}

pub fn extra_map_as<T>(extra: &Map<String, Value>, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    extra_map_value(extra, key).and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn extra_map_value<'a>(extra: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    extra.get(key).or_else(|| {
        extra
            .get("extra")
            .and_then(Value::as_object)
            .and_then(|extra| extra.get(key))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_meta_extra_as_reads_flattened_extra() {
        let meta: RequestMeta = serde_json::from_value(json!({
            "source": "browser:chrome:1",
            "conversation": 42,
        }))
        .expect("request meta should deserialize");

        assert_eq!(
            request_meta_extra_as::<String>(&meta, "source"),
            Some("browser:chrome:1".to_string())
        );
        assert_eq!(
            request_meta_extra_as::<u64>(&meta, "conversation"),
            Some(42)
        );
    }

    #[test]
    fn request_meta_extra_as_reads_nested_extra() {
        let meta: RequestMeta = serde_json::from_value(json!({
            "extra": {
                "source": "browser:chrome:1",
                "conversation": 42,
                "workspace": "/tmp/browser",
            }
        }))
        .expect("legacy request meta should deserialize");

        assert_eq!(meta.get_extra_as::<String>("source"), None);
        assert_eq!(
            request_meta_extra_as::<String>(&meta, "source"),
            Some("browser:chrome:1".to_string())
        );
        assert_eq!(
            request_meta_extra_as::<u64>(&meta, "conversation"),
            Some(42)
        );
        assert_eq!(
            request_meta_extra_as::<String>(&meta, "workspace"),
            Some("/tmp/browser".to_string())
        );
    }
}
