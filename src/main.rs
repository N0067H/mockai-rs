mod app;
mod chat;
mod config;
mod embeddings;
mod error;
mod models;
mod responses;
mod test_data;

use clap::Parser;
use config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = Config::parse();
    let data = test_data::TestData::load(config.data_file.as_deref())?;
    let app = app::router(config.api_key.clone(), data);
    let listener = TcpListener::bind(config.address()).await?;

    println!("mockai-rs listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await
}
