use serde;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WireMessage {
    error: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    data: serde_json::Value,
}

type WireResult = Result<serde_json::Value, String>;

pub fn serialize<S>(result: &WireResult, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let wire = match result {
        Ok(value) => WireMessage {
            error: "success".to_string(),
            data: value.clone(),
        },
        Err(error) => WireMessage {
            error: error.to_string(),
            data: serde_json::Value::Null,
        },
    };
    wire.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<WireResult, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = WireMessage::deserialize(deserializer)?;
    let result = if &wire.error == "success" {
        Ok(wire.data)
    } else {
        Err(wire.error)
    };
    Ok(result)
}
