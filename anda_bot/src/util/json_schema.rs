#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
pub fn assert_openai_strict_parameters(parameters: &Value) {
    assert_openai_strict_schema(parameters, "$parameters");
}

#[cfg(test)]
fn assert_openai_strict_schema(schema: &Value, path: &str) {
    let Some(object) = schema.as_object() else {
        return;
    };

    for keyword in ["anyOf", "oneOf", "allOf", "default"] {
        assert!(
            !object.contains_key(keyword),
            "{path} contains unsupported schema keyword {keyword}"
        );
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        assert_eq!(
            object.get("additionalProperties"),
            Some(&Value::Bool(false)),
            "{path} must set additionalProperties to false"
        );

        let property_keys = properties.keys().cloned().collect::<BTreeSet<_>>();
        let required_keys = object
            .get("required")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{path} must define required"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("{path}.required must contain only strings"))
                    .to_string()
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(
            required_keys, property_keys,
            "{path}.required must include every property key"
        );

        for (name, property) in properties {
            assert_openai_strict_schema(property, &format!("{path}.properties.{name}"));
        }
    }

    if let Some(items) = object.get("items") {
        assert_openai_strict_schema(items, &format!("{path}.items"));
    }
}
