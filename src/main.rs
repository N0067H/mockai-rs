mod config;

use axum::{Router, routing::get};
use clap::Parser;
use config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = Config::parse();
    let app = Router::new().route("/", get(|| async { "mockai-rs\n" }));
    let listener = TcpListener::bind(config.address()).await?;

    println!("mockai-rs listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await
}
