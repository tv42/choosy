use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub filename: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub items: Vec<SearchResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayCommand {
    pub filename: String,
}
