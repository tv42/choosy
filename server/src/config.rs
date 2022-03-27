use serde::Deserialize;
use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename = "ChoosyConfig")]
pub struct Config {
    pub path: String,
    /// Whether the player should be fullscreen or not.
    #[serde(default = "default_true")]
    pub fullscreen: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("error reading: {source}")]
    IO {
        #[from]
        source: std::io::Error,
    },
    #[error("error parsing: {source}")]
    Parse {
        #[from]
        source: ron::Error,
    },
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
        let file = File::open(path)?;
        let config: Config = ron::de::from_reader(file)?;
        Ok(config)
    }
}
