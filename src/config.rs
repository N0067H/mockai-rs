use std::net::{IpAddr, SocketAddr};

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "A local mock server for the OpenAI API")]
pub struct Config {
    #[arg(long, env = "MOCKAI_HOST", default_value = "127.0.0.1")]
    host: IpAddr,

    #[arg(long, env = "MOCKAI_PORT", default_value_t = 58881)]
    port: u16,
}

impl Config {
    pub fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}
