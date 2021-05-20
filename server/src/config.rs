use async_std::io;
use serde::Deserialize;
use std::fs::File;
use thiserror::Error;

#[derive(Deserialize, Debug)]
#[serde(rename = "ChoosyConfig")]
pub struct Config {
    pub path: String,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("error reading: {source}")]
    IO {
        #[from]
        source: io::Error,
    },
    #[error("error parsing: {source}")]
    Parse {
        #[from]
        source: ron::Error,
    },
}

impl Config {
    pub fn load(filename: &str) -> Result<Config, ConfigError> {
        let file = File::open(filename)?;
        let config: Config = ron::de::from_reader(file)?;
        Ok(config)
    }
}
