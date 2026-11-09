//! The hub's inbound connections are bounded.
//!
//! Each connection holds a task and a descriptor for its whole life, and hyper
//! re-arms `header_read_timeout` on every keep-alive request, so one request
//! every 29 s holds a connection indefinitely. Uncapped, enough of them took
//! every descriptor the process had, and the flush's publish dials then failed
//! as EMFILE (external report, @ouicate).
//!
//! Every held connection here has completed a keep-alive `GET /healthz` first.
//! That is the attack: a served connection that stays open, and not merely a
//! socket that never spoke.
//!
//! **How refused is told apart from served.** An HTTP/1.1 server sends nothing
//! until it has a request. So a served connection blocks on read, and a refused
//! one reads EOF at once.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use zero_indexer_hub::batcher::{BatchParams, TipGuard, TipTracker};
use zero_indexer_hub::chain::ChainClient;
use zero_indexer_hub::queue::Queue;
use zero_indexer_hub::server::{self, Hub, ServeOptions};

/// Mirrors `server::MAX_INFLIGHT_CONNECTIONS`, which is deliberately private: an
/// operator who could raise it could re-open the hole.
const LIMIT: usize = 256;

const HEALTHZ: &[u8] = b"GET /healthz HTTP/1.1\r\nhost: hub\r\n\r\n";

async fn spawn_hub() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let tip = Arc::new(TipTracker::new(TipGuard::Unguarded));
    tip.observe(&[100]);
    tokio::spawn(server::serve(
        listener,
        Hub::new(
            Arc::new(Queue::new()),
            tip,
            BatchParams::default(),
            // Constructed, never connected: nothing here publishes or looks up.
            Arc::new(ChainClient::new(vec!["127.0.0.1:9".parse().unwrap()], None).unwrap()),
        ),
        ServeOptions::default(),
    ));
    addr
}

/// Send `GET /healthz` and read the whole `200 ok` reply, leaving the
/// connection open for the next request.
async fn healthz(stream: &mut TcpStream) -> bool {
    if stream.write_all(HEALTHZ).await.is_err() {
        return false;
    }
    let mut reply = Vec::new();
    let mut buf = [0u8; 512];
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return false,
                Ok(n) => reply.extend_from_slice(&buf[..n]),
            }
            if reply.starts_with(b"HTTP/1.1 200") && reply.ends_with(b"\r\n\r\nok") {
                return true;
            }
        }
    })
    .await;
    read.unwrap_or(false)
}

/// Open `n` connections and complete one keep-alive request on each.
async fn hold_served(hub: SocketAddr, n: usize) -> Vec<TcpStream> {
    let mut held = Vec::with_capacity(n);
    for _ in 0..n {
        let Ok(mut stream) = TcpStream::connect(hub).await else {
            break;
        };
        if !healthz(&mut stream).await {
            break;
        }
        held.push(stream);
    }
    held
}

#[tokio::test]
async fn a_connection_over_the_limit_is_closed_while_keep_alive_connections_hold_every_slot() {
    let hub = spawn_hub().await;

    let mut held = hold_served(hub, LIMIT).await;
    assert_eq!(
        held.len(),
        LIMIT,
        "precondition: every slot was taken by a served connection"
    );

    // Still served, a request later: keep-alive is what holds the slot.
    assert!(
        healthz(&mut held[0]).await,
        "a held connection stopped answering: the test no longer exercises keep-alive"
    );

    let mut over = TcpStream::connect(hub).await.expect("connect");
    let mut buf = [0u8; 1];
    match tokio::time::timeout(Duration::from_secs(5), over.read(&mut buf)).await {
        Ok(Ok(0)) | Ok(Err(_)) => {} // EOF or reset: refused, as intended
        Ok(Ok(n)) => panic!("an HTTP/1.1 server sent {n} unsolicited bytes"),
        Err(_) => panic!(
            "a connection over the limit was HELD OPEN for 5s: the bound is not applied, or it \
             parks connections instead of closing them"
        ),
    }

    drop(held);
}

#[tokio::test]
async fn slots_are_returned_when_connections_close() {
    // A bound that never released would be its own denial of service.
    let hub = spawn_hub().await;

    let held = hold_served(hub, LIMIT).await;
    assert_eq!(
        held.len(),
        LIMIT,
        "precondition: every slot was taken by a served connection"
    );
    drop(held);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut fresh = TcpStream::connect(hub).await.expect("connect");
    assert!(
        healthz(&mut fresh).await,
        "a connection was refused after every held slot had closed: permits are not released"
    );
}
