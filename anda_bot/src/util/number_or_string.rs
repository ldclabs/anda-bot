//! Tool arguments that accept an unsigned integer as a JSON number or a
//! numeric string, since models often quote ids and limits.

use serde::{Deserialize, Deserializer, de};
use std::{fmt::Display, str::FromStr};

#[derive(Deserialize)]
#[serde(untagged)]
enum NumberOrString<T> {
    Number(T),
    String(String),
}

impl<T> NumberOrString<T>
where
    T: FromStr,
    T::Err: Display,
{
    fn parse(self) -> Result<T, String> {
        match self {
            Self::Number(value) => Ok(value),
            Self::String(value) => {
                let value = value.trim();
                if value.is_empty() {
                    return Err("expected a non-empty unsigned integer string".to_string());
                }
                value
                    .parse()
                    .map_err(|err| format!("invalid unsigned integer '{value}': {err}"))
            }
        }
    }
}

/// `#[serde(deserialize_with = "number_or_string::deserialize")]`
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    T::Err: Display,
{
    NumberOrString::<T>::deserialize(deserializer)?
        .parse()
        .map_err(de::Error::custom)
}

/// `#[serde(default, deserialize_with = "number_or_string::deserialize_optional")]`
pub fn deserialize_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    T::Err: Display,
{
    Option::<NumberOrString<T>>::deserialize(deserializer)?
        .map(NumberOrString::parse)
        .transpose()
        .map_err(de::Error::custom)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize)]
    struct Args {
        #[serde(deserialize_with = "super::deserialize")]
        id: u64,
        #[serde(default, deserialize_with = "super::deserialize_optional")]
        limit: Option<usize>,
    }

    #[test]
    fn accepts_numbers_and_numeric_strings() {
        let args: Args = serde_json::from_value(json!({"id": "7", "limit": 25})).unwrap();
        assert_eq!((args.id, args.limit), (7, Some(25)));
        let args: Args = serde_json::from_value(json!({"id": 7, "limit": " 3 "})).unwrap();
        assert_eq!((args.id, args.limit), (7, Some(3)));
        let args: Args = serde_json::from_value(json!({"id": 7})).unwrap();
        assert_eq!(args.limit, None);

        let err = serde_json::from_value::<Args>(json!({"id": "  "}))
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("non-empty unsigned integer"));
        let err = serde_json::from_value::<Args>(json!({"id": "abc"}))
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("invalid unsigned integer"));
    }
}
