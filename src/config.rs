use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "A local mock server for the OpenAI API")]
pub struct Config {
    #[arg(long, env = "MOCKAI_HOST", default_value = "127.0.0.1")]
    host: IpAddr,

    #[arg(long, env = "MOCKAI_PORT", default_value_t = 58881)]
    port: u16,

    #[arg(long, env = "MOCKAI_API_KEY", default_value = "mock-api-key", hide_env_values = true, value_parser = parse_api_key)]
    pub api_key: String,

    /// JSON file with test models and fixed replies
    #[arg(long, env = "MOCKAI_DATA_FILE")]
    pub data_file: Option<PathBuf>,
}

fn parse_api_key(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("test key must be non-empty ASCII text with no spaces".into());
    }
    Ok(value.to_owned())
}

impl Config {
    pub fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}
