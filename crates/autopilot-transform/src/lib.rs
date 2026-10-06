use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TransformOp {
    GetField { field: String },
    Index { index: usize },
    First,
    MapField { field: String },
    FilterEq { field: String, value: Value },
    ParseJson { output_schema: Value },
    JoinStrings { separator: String },
    AssertString { min_length: usize, max_length: Option<usize> },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerifiedTransform {
    #[serde(default)]
    pub ops: Vec<TransformOp>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum TransformError {
    #[error("transform expected object for field {0}")]
    ExpectedObject(String),
    #[error("transform field is not declared by schema: {0}")]
    UnknownField(String),
    #[error("transform expected array")]
    ExpectedArray,
    #[error("transform index {index} is not guaranteed by schema")]
    UnsafeIndex { index: usize },
    #[error("transform expected string")]
    ExpectedString,
    #[error("transform filter literal is incompatible with field schema: {0}")]
    InvalidFilterLiteral(String),
    #[error("transform runtime field is missing: {0}")]
    MissingField(String),
    #[error("transform runtime index {0} is out of range")]
    IndexOutOfRange(usize),
    #[error("transform JSON parse failed: {0}")]
    InvalidJson(String),
    #[error("transform runtime value violates declared output schema: {0}")]
    OutputContract(String),
}

impl VerifiedTransform {
    pub fn infer_output_schema(&self, input_schema: &Value) -> Result<Value, TransformError> {
        let mut schema = input_schema.clone();
        for op in &self.ops {
            schema = infer_one(&schema, op)?;
        }
        Ok(schema)
    }

    pub fn apply(&self, input: Value) -> Result<Value, TransformError> {
        let mut value = input;
        for op in &self.ops {
            value = apply_one(value, op)?;
        }
        Ok(value)
    }
}

fn infer_one(schema: &Value, op: &TransformOp) -> Result<Value, TransformError> {
    match op {
        TransformOp::GetField { field } => {
            if schema_type(schema) != Some("object") {
                return Err(TransformError::ExpectedObject(field.clone()));
            }
            schema.get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get(field))
                .cloned()
                .ok_or_else(|| TransformError::UnknownField(field.clone()))
        }
        TransformOp::Index { index } => {
            if schema_type(schema) != Some("array") {
                return Err(TransformError::ExpectedArray);
            }
            let min_items = schema.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize;
            if min_items <= *index {
                return Err(TransformError::UnsafeIndex { index: *index });
            }
            schema.get("items").cloned().ok_or(TransformError::ExpectedArray)
        }
        TransformOp::First => {
            if schema_type(schema) != Some("array") {
                return Err(TransformError::ExpectedArray);
            }
            let min_items = schema.get("minItems").and_then(Value::as_u64).unwrap_or(0);
            if min_items < 1 {
                return Err(TransformError::UnsafeIndex { index: 0 });
            }
            schema.get("items").cloned().ok_or(TransformError::ExpectedArray)
        }
        TransformOp::MapField { field } => {
            if schema_type(schema) != Some("array") {
                return Err(TransformError::ExpectedArray);
            }
            let items = schema.get("items").ok_or(TransformError::ExpectedArray)?;
            if schema_type(items) != Some("object") {
                return Err(TransformError::ExpectedObject(field.clone()));
            }
            let child = items.get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get(field))
                .cloned()
                .ok_or_else(|| TransformError::UnknownField(field.clone()))?;
            let mut out = serde_json::json!({"type":"array","items":child});
            if let Some(min) = schema.get("minItems") { out["minItems"] = min.clone(); }
            if let Some(max) = schema.get("maxItems") { out["maxItems"] = max.clone(); }
            Ok(out)
        }
        TransformOp::FilterEq { field, value } => {
            if schema_type(schema) != Some("array") {
                return Err(TransformError::ExpectedArray);
            }
            let items = schema.get("items").ok_or(TransformError::ExpectedArray)?;
            let field_schema = items.get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get(field))
                .ok_or_else(|| TransformError::UnknownField(field.clone()))?;
            if !literal_matches_schema(value, field_schema) {
                return Err(TransformError::InvalidFilterLiteral(field.clone()));
            }
            let mut out = schema.clone();
            if let Some(object) = out.as_object_mut() {
                object.insert("minItems".into(), Value::from(0));
            }
            Ok(out)
        }
        TransformOp::ParseJson { output_schema } => {
            if schema_type(schema) != Some("string") {
                return Err(TransformError::ExpectedString);
            }
            Ok(output_schema.clone())
        }
        TransformOp::JoinStrings { .. } => {
            if schema_type(schema) != Some("array")
                || schema.get("items").and_then(schema_type) != Some("string")
            {
                return Err(TransformError::ExpectedString);
            }
            Ok(serde_json::json!({"type":"string"}))
        }
        TransformOp::AssertString { min_length, max_length } => {
            if schema_type(schema) != Some("string") {
                return Err(TransformError::ExpectedString);
            }
            if let Some(max) = max_length {
                if *max < *min_length {
                    return Err(TransformError::OutputContract(
                        "assert_string max_length is below min_length".into()
                    ));
                }
            }
            let mut out = schema.clone();
            let object = out.as_object_mut().ok_or(TransformError::ExpectedString)?;
            let existing_min = object.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize;
            object.insert("minLength".into(), Value::from(existing_min.max(*min_length) as u64));
            if let Some(max) = max_length {
                let effective = object.get("maxLength").and_then(Value::as_u64)
                    .map(|existing| existing.min(*max as u64))
                    .unwrap_or(*max as u64);
                object.insert("maxLength".into(), Value::from(effective));
            }
            Ok(out)
        }
    }
}

fn apply_one(value: Value, op: &TransformOp) -> Result<Value, TransformError> {
    match op {
        TransformOp::GetField { field } => value.as_object()
            .and_then(|object| object.get(field))
            .cloned()
            .ok_or_else(|| TransformError::MissingField(field.clone())),
        TransformOp::Index { index } => value.as_array()
            .and_then(|items| items.get(*index))
            .cloned()
            .ok_or(TransformError::IndexOutOfRange(*index)),
        TransformOp::First => value.as_array()
            .and_then(|items| items.first())
            .cloned()
            .ok_or(TransformError::IndexOutOfRange(0)),
        TransformOp::MapField { field } => {
            let items = value.as_array().ok_or(TransformError::ExpectedArray)?;
            items.iter().map(|item| {
                item.as_object()
                    .and_then(|object| object.get(field))
                    .cloned()
                    .ok_or_else(|| TransformError::MissingField(field.clone()))
            }).collect::<Result<Vec<_>,_>>().map(Value::Array)
        }
        TransformOp::FilterEq { field, value: expected } => {
            let items = value.as_array().ok_or(TransformError::ExpectedArray)?;
            Ok(Value::Array(items.iter().filter(|item| {
                item.as_object().and_then(|object| object.get(field)) == Some(expected)
            }).cloned().collect()))
        }
        TransformOp::ParseJson { output_schema } => {
            let text = value.as_str().ok_or(TransformError::ExpectedString)?;
            let parsed: Value = serde_json::from_str(text)
                .map_err(|error| TransformError::InvalidJson(error.to_string()))?;
            let errors = validate_basic(output_schema, &parsed, "$");
            if errors.is_empty() { Ok(parsed) }
            else { Err(TransformError::OutputContract(errors.join("; "))) }
        }
        TransformOp::JoinStrings { separator } => {
            let items = value.as_array().ok_or(TransformError::ExpectedArray)?;
            let strings = items.iter().map(Value::as_str)
                .collect::<Option<Vec<_>>>()
                .ok_or(TransformError::ExpectedString)?;
            Ok(Value::String(strings.join(separator)))
        }
        TransformOp::AssertString { min_length, max_length } => {
            let text = value.as_str().ok_or(TransformError::ExpectedString)?;
            let len = text.chars().count();
            if len < *min_length {
                return Err(TransformError::OutputContract(format!(
                    "string length {len} is below asserted minimum {min_length}"
                )));
            }
            if let Some(max) = max_length {
                if len > *max {
                    return Err(TransformError::OutputContract(format!(
                        "string length {len} exceeds asserted maximum {max}"
                    )));
                }
            }
            Ok(value)
        }
    }
}

fn schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str).or_else(|| {
        if schema.get("properties").is_some() { Some("object") }
        else if schema.get("items").is_some() { Some("array") }
        else { None }
    })
}

fn literal_matches_schema(value: &Value, schema: &Value) -> bool {
    if let Some(constant) = schema.get("const") { return value == constant; }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values.iter().any(|candidate| candidate == value);
    }
    match schema_type(schema) {
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("null") => value.is_null(),
        _ => true,
    }
}

fn validate_basic(schema: &Value, value: &Value, path: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(kind) = schema_type(schema) {
        let ok = match kind {
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            "null" => value.is_null(),
            _ => true,
        };
        if !ok {
            errors.push(format!("{path}: expected {kind}"));
            return errors;
        }
    }
    if let Some(constant) = schema.get("const") {
        if value != constant { errors.push(format!("{path}: const mismatch")); }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.iter().any(|candidate| candidate == value) {
            errors.push(format!("{path}: enum mismatch"));
        }
    }
    if let (Some(required), Some(object)) = (
        schema.get("required").and_then(Value::as_array),
        value.as_object(),
    ) {
        for field in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                errors.push(format!("{path}.{field}: required"));
            }
        }
    }
    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        for (field, child) in object {
            if let Some(child_schema) = properties.get(field) {
                errors.extend(validate_basic(child_schema, child, &format!("{path}.{field}")));
            }
        }
    }
    if let (Some(items_schema), Some(items)) = (schema.get("items"), value.as_array()) {
        for (index, item) in items.iter().enumerate() {
            errors.extend(validate_basic(items_schema, item, &format!("{path}[{index}]")));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_projection_requires_proven_index() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{
                "results":{
                    "type":"array",
                    "minItems":2,
                    "items":{
                        "type":"object",
                        "properties":{"text":{"type":"string"}},
                        "required":["text"]
                    }
                }
            },
            "required":["results"]
        });
        let transform = VerifiedTransform { ops: vec![
            TransformOp::GetField { field:"results".into() },
            TransformOp::Index { index:1 },
            TransformOp::GetField { field:"text".into() },
        ]};
        assert_eq!(
            transform.infer_output_schema(&schema).unwrap(),
            serde_json::json!({"type":"string"})
        );
        let value = serde_json::json!({"results":[{"text":"a"},{"text":"b"}]});
        assert_eq!(transform.apply(value).unwrap(), Value::String("b".into()));
    }

    #[test]
    fn unsafe_first_is_rejected_without_min_items() {
        let schema = serde_json::json!({"type":"array","items":{"type":"string"}});
        let transform = VerifiedTransform { ops: vec![TransformOp::First] };
        assert!(matches!(
            transform.infer_output_schema(&schema),
            Err(TransformError::UnsafeIndex { index:0 })
        ));
    }

    #[test]
    fn assert_string_strengthens_contract_and_runtime() {
        let transform = VerifiedTransform { ops: vec![
            TransformOp::AssertString { min_length:1, max_length:Some(10) }
        ]};
        assert_eq!(
            transform.infer_output_schema(&serde_json::json!({"type":"string"})).unwrap(),
            serde_json::json!({"type":"string","minLength":1,"maxLength":10})
        );
        assert!(transform.apply(Value::String(String::new())).is_err());
        assert_eq!(
            transform.apply(Value::String("hello".into())).unwrap(),
            Value::String("hello".into())
        );
    }

    #[test]
    fn map_filter_and_join_are_deterministic() {
        let schema = serde_json::json!({
            "type":"array","minItems":1,
            "items":{
                "type":"object",
                "properties":{
                    "kind":{"type":"string","enum":["keep","drop"]},
                    "text":{"type":"string"}
                }
            }
        });
        let transform = VerifiedTransform { ops: vec![
            TransformOp::FilterEq { field:"kind".into(), value:Value::String("keep".into()) },
            TransformOp::MapField { field:"text".into() },
            TransformOp::JoinStrings { separator:"\n".into() },
        ]};
        assert_eq!(transform.infer_output_schema(&schema).unwrap(), serde_json::json!({"type":"string"}));
        let input = serde_json::json!([
            {"kind":"keep","text":"a"},
            {"kind":"drop","text":"b"},
            {"kind":"keep","text":"c"}
        ]);
        assert_eq!(transform.apply(input).unwrap(), Value::String("a\nc".into()));
    }
}
