//! Valkey/Redis resilience: the connection pool and commands must fail fast,
//! not hang, when Valkey stalls.

use deadpool_redis::redis;
use farms::{configuration::RedisSettings, startup::get_redis_connection_pool};
use secrecy::SecretString;
use std::{future::Future, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Upper bound for an operation that must fail fast. Well above the 1s
/// configured timeout, but finite, so a missing timeout fails the test
/// instead of hanging the suite.
const HANG_GUARD: Duration = Duration::from_secs(10);

async fn must_finish<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(HANG_GUARD, future)
        .await
        .expect("operation hung: no Redis timeout was applied")
}

fn redis_settings(uri: String, pool_max_size: usize) -> RedisSettings {
    RedisSettings {
        uri: SecretString::from(uri),
        pool_max_size: Some(pool_max_size),
        timeout_seconds: Some(1),
        session_key_prefix: "sess".to_string(),
    }
}

/// Accepts TCP connections and never writes a byte back, like a Valkey that
/// is up at the network level but stalled. Sockets are kept open so the
/// client sees silence rather than a reset.
async fn spawn_silent_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    format!("redis://{address}")
}

/// Answers `+OK` to every command (enough for the client's connection
/// handshake) until it receives one containing `STALL`, then goes silent on
/// that connection.
async fn spawn_stalling_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                loop {
                    let read = match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => read,
                    };
                    let chunk = &buffer[..read];
                    if chunk.windows(5).any(|window| window == b"STALL") {
                        // Keep the socket open, never reply.
                        std::future::pending::<()>().await;
                    }
                    // One RESP array header (`*<n>\r\n` at a line start) per command.
                    let commands = chunk
                        .iter()
                        .enumerate()
                        .filter(|&(i, &byte)| byte == b'*' && (i == 0 || chunk[i - 1] == b'\n'))
                        .count();
                    for _ in 0..commands {
                        if socket.write_all(b"+OK\r\n").await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    format!("redis://{address}")
}

#[tokio::test]
async fn pool_get_times_out_when_valkey_never_answers() {
    let pool = get_redis_connection_pool(&redis_settings(spawn_silent_server().await, 2)).unwrap();

    let result = must_finish(pool.get()).await;

    assert!(
        result.is_err(),
        "Creating a connection to a silent server must time out"
    );
}

#[tokio::test]
async fn command_times_out_when_valkey_stops_answering() {
    let pool =
        get_redis_connection_pool(&redis_settings(spawn_stalling_server().await, 2)).unwrap();
    let mut connection = must_finish(pool.get())
        .await
        .expect("The handshake is answered, so a connection should be created");

    let result = must_finish(
        redis::cmd("GET")
            .arg("STALL")
            .query_async::<Option<String>>(&mut connection),
    )
    .await;

    assert!(
        result.is_err(),
        "A command the server never answers must time out"
    );
}

#[tokio::test]
async fn pool_get_times_out_when_pool_is_exhausted() {
    let pool =
        get_redis_connection_pool(&redis_settings(spawn_stalling_server().await, 1)).unwrap();
    let _only_connection = must_finish(pool.get())
        .await
        .expect("The first connection should be created");

    let result = must_finish(pool.get()).await;

    assert!(
        result.is_err(),
        "Waiting for a free connection must time out"
    );
}
