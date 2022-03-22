// TODO these types may be public, but they are most definitely not stable. exhaustive match arms will need updating regularly, and if we get Unknown variants working, things will move from Unknown to strongly-typed variants over time.

use serde::{Deserialize, Serialize};
use std::time;

mod serde_duration;
mod serde_ipcresult;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "name")]
#[serde(rename_all = "kebab-case")]
pub enum PropertyChange {
    TimePos {
        #[serde(rename = "data")]
        #[serde(with = "self::serde_duration")]
        seconds: time::Duration,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "event")]
#[serde(rename_all = "kebab-case")]
pub enum MPVEventKind {
    StartFile { playlist_entry_id: u64 },
    PropertyChange(PropertyChange),
    // WAITING add an "Unknown" type that contains name and serde_json::Value. currently cannot capture content without huge kludges. <https://github.com/serde-rs/serde/issues/912>. also change enum PropertyChange.
}

/// This is only used for serialize
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(num: &u64) -> bool {
    *num == 0
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MPVEvent {
    #[serde(skip_serializing_if = "is_zero")]
    #[serde(default)]
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    error: Option<String>,
    #[serde(flatten)]
    event: MPVEventKind,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Response {
    pub request_id: u64,
    // We're assuming MPV IPC errors never contain both an error message and data.
    #[serde(with = "self::serde_ipcresult")]
    #[serde(flatten)]
    pub result: Result<serde_json::Value, String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum MPVEnvelope {
    Event(MPVEvent),
    Response(Response),
}

/// This is only used for serialize
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    b == &false
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Command {
    // TODO this should really tie more into async==false, but I can't reach that from the helper. maybe this should become an Option, or Command an enum.
    #[serde(skip_serializing_if = "is_zero")]
    #[serde(default)]
    pub request_id: u64,
    pub command: serde_json::Value,
    #[serde(rename = "async")]
    #[serde(skip_serializing_if = "is_false")]
    #[serde(default)]
    pub async_: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn check(json: serde_json::Value, env: MPVEnvelope) {
        // serialize
        let output = serde_json::to_string(&env).unwrap();
        let value: serde_json::Value = output.parse().unwrap();
        assert_eq!(value, json, "serialize");

        // deserialize
        let parsed: MPVEnvelope = serde_json::from_str(&output).unwrap();
        assert_eq!(env, parsed, "deserialize");
    }

    #[test]
    fn event_start_file() {
        check(
            json!({
                "event": "start-file",
                "playlist_entry_id": 123,
            }),
            MPVEnvelope::Event(MPVEvent {
                id: 0,
                error: None,
                event: MPVEventKind::StartFile {
                    playlist_entry_id: 123,
                },
            }),
        );
    }

    #[test]
    fn event_property_time_pos() {
        check(
            json!({
                "event":"property-change",
                "id":42,
                "name":"time-pos",
                "data":39.489450,
            }),
            MPVEnvelope::Event(MPVEvent {
                id: 42,
                error: None,
                event: MPVEventKind::PropertyChange(PropertyChange::TimePos {
                    seconds: time::Duration::from_secs_f64(39.489450),
                }),
            }),
        );
    }

    #[test]
    fn response_success_bare() {
        check(
            json!({"request_id":1,"error":"success"}),
            MPVEnvelope::Response(Response {
                request_id: 1,
                result: Ok(json!(serde_json::Value::Null)),
            }),
        )
    }

    #[test]
    fn response_success_data() {
        check(
            json!({"data":32.649283,"request_id":0,"error":"success"}),
            MPVEnvelope::Response(Response {
                request_id: 0,
                result: Ok(json!(32.649283)),
                // error: "success".to_string(),
                // data: json!(32.649283),
            }),
        )
    }

    // fn event_property_unknown() {
    //     check(
    //         MPVEnvelope::Event(MPVEvent {
    //             id: 13,
    //             error: None,
    //             event: MPVEventKind::Unknown {
    //                 name: "test-trigger-unknown".to_string(),
    //                 data: json!({"xyzzy": "foo"}),
    //             },
    //         }),
    //         json!({"event":"property-change","id":13,"name":"test-trigger-unknown","data":{"xyzzy": "foo"}}),
    //     );
    // }

    fn check_command(json: serde_json::Value, command: Command) {
        // serialize
        let output = serde_json::to_string(&command).unwrap();
        let value: serde_json::Value = output.parse().unwrap();
        assert_eq!(value, json, "serialize");

        // deserialize
        let parsed: Command = serde_json::from_str(&output).expect("must parse");
        assert_eq!(command, parsed, "deserialize");
    }

    #[test]
    fn command() {
        check_command(
            json!(    {"command":["observe_property",42,"playback-time"]}),
            Command {
                request_id: 0,
                command: json!(["observe_property", 42, "playback-time"]),
                async_: false,
            },
        )
    }
}

// TODO event: unpause playback-restart seek

// {"event":"end-file","reason":"quit","playlist_entry_id":1}

// updates every 0.02 seconds
//
// TODO observe id. just use id 0?, we're never unregistering
