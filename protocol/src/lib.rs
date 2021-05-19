use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WSEvent {
    FileChange(Vec<FileChange>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum FileChange {
    ClearAll,
    Add { name: String },
    Del { name: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WSCommand {
    Play { filename: String },
}
