mod app;
mod config;
mod error;

use clap::Parser;
use config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = Config::parse();
    let app = app::router(config.api_key.clone());
    let listener = TcpListener::bind(config.address()).await?;

    println!("mockai-rs listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await
}
