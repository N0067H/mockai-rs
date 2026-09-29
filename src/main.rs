use axum::{Router, routing::get};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = Router::new().route("/", get(|| async { "mockai-rs\n" }));
    let listener = TcpListener::bind("127.0.0.1:58881").await?;

    println!("mockai-rs listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await
}
