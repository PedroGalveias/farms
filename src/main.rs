use farms::run;
use std::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");

    // Bubble up the io::Error if we failed to bind the address
    // Otherwise call .await on Server
    run(listener)?.await
}
