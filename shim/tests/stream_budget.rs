//! Buffered request bodies share one byte budget across every connection and
//! every stream.
//!
//! The connection bound (`inbound_bound.rs`) did not bound memory, because
//! HTTP/2 multiplies it: each connection carries many streams, and each
//! `SendTransaction` stream buffered up to 4 MiB for up to 30 s. One connection
//! pinned about 800 MiB (external report, @ouicate).
//!
//! These tests drive that multiplier directly: three connections, every stream
//! each may open (32, `proxy::MAX_CONCURRENT_STREAMS`), each stream sending a
//! full 4 MiB body and never finishing it. One connection holds at most 128 MiB
//! of bodies, so the budget bounds the total across connections. The budget
//! (256 MiB, `intercept::BODY_BUDGET_BYTES`) holds at most 64 such bodies, so a
//! 96-stream flood must be refused in part, promptly, as UNAVAILABLE. Before
//! the budget every stream waited out the 30 s read timeout instead.
//!
//! This file is its own test binary, so its floods share the budget with
//! nothing else.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::Request;
use http_body::Frame;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, StreamBody};
use hyper::client::conn::http2 as client_h2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

mod common;
use common::{
    connect_h2, send_tx_reply, spawn_counting_backend, spawn_forward_only_shim, V6_IRONWOOD_ONLY,
};
use zero_indexer_shim::proxy::SEND_TRANSACTION;

/// Mirrors `proxy::MAX_CONCURRENT_STREAMS`: every stream one connection may open.
const STREAMS: usize = 32;

/// Connections in one flood: three connections' streams pass the budget.
const CONNECTIONS: usize = 3;

/// Mirrors `intercept::MAX_SEND_TX_BYTES`: the most one body may buffer.
const BODY: usize = 4 * 1024 * 1024;

/// Mirrors `intercept::BODY_BUDGET_BYTES` / `BODY`: the full bodies it holds.
const BODIES_IN_BUDGET: usize = 64;

/// Well inside the shim's 30 s body-read timeout, so a reply in time is the
/// budget's answer and never the timeout's.
const DEADLINE: Duration = Duration::from_secs(25);

const GRPC_UNAVAILABLE: i32 = 14;

/// An HTTP/2 connection whose driver task the caller holds, so a test can drop
/// the connection with every stream on it.
async fn connect(
    shim: SocketAddr,
) -> (
    client_h2::SendRequest<BoxBody<Bytes, Infallible>>,
    JoinHandle<()>,
) {
    let stream = TcpStream::connect(shim).await.unwrap();
    let (sender, conn) = client_h2::Builder::new(TokioExecutor::new())
        .handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let driver = tokio::spawn(async move {
        let _ = conn.await;
    });
    (sender, driver)
}

/// A body that delivers `bytes` and then stays open, as an attacker's does.
fn held_body(bytes: Bytes) -> BoxBody<Bytes, Infallible> {
    let frames =
        futures_util::stream::iter([Ok(Frame::data(bytes))]).chain(futures_util::stream::pending());
    BodyExt::boxed(StreamBody::new(frames))
}

/// Open `n` SendTransaction streams on one connection, each delivering `bytes`
/// and never finishing. Every reply's gRPC status arrives on `tx`.
async fn flood(
    sender: &mut client_h2::SendRequest<BoxBody<Bytes, Infallible>>,
    shim: SocketAddr,
    n: usize,
    bytes: Bytes,
    tx: &mpsc::UnboundedSender<i32>,
) -> Vec<JoinHandle<()>> {
    let mut streams = Vec::with_capacity(n);
    for _ in 0..n {
        let request = Request::builder()
            .method("POST")
            .uri(format!("http://{shim}{SEND_TRANSACTION}"))
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .body(held_body(bytes.clone()))
            .unwrap();
        sender.ready().await.unwrap();
        let reply = sender.send_request(request);
        let tx = tx.clone();
        streams.push(tokio::spawn(async move {
            if let Ok(response) = reply.await {
                let status = response
                    .headers()
                    .get("grpc-status")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let _ = tx.send(status);
            }
        }));
    }
    streams
}

/// Collect replies until at least `want` have arrived or the deadline passes,
/// then a short settle so a cascade of refusals has time to land.
async fn replies(rx: &mut mpsc::UnboundedReceiver<i32>, want: usize) -> Vec<i32> {
    let mut statuses = Vec::new();
    let _ = tokio::time::timeout(DEADLINE, async {
        while statuses.len() < want {
            match rx.recv().await {
                Some(status) => statuses.push(status),
                None => break,
            }
        }
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    while let Ok(status) = rx.try_recv() {
        statuses.push(status);
    }
    statuses
}

/// Flood `CONNECTIONS` connections and check the budget's answer. Returns the
/// connections' drivers and streams so the caller decides when they end.
async fn flood_connections(shim: SocketAddr) -> (Vec<JoinHandle<()>>, Vec<JoinHandle<()>>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let body = Bytes::from(vec![0u8; BODY]);
    let mut senders = Vec::new();
    let mut drivers = Vec::new();
    let mut streams = Vec::new();
    for _ in 0..CONNECTIONS {
        let (mut sender, driver) = connect(shim).await;
        streams.extend(flood(&mut sender, shim, STREAMS, body.clone(), &tx).await);
        senders.push(sender);
        drivers.push(driver);
    }

    let total = CONNECTIONS * STREAMS;
    let over = total - BODIES_IN_BUDGET;
    let statuses = replies(&mut rx, over).await;
    assert!(
        statuses.len() >= over,
        "{total} streams of {BODY} bytes on {CONNECTIONS} connections got {} replies within \
         {DEADLINE:?}; at least {over} must be refused, or the shim buffers more than the budget",
        statuses.len()
    );
    assert!(
        statuses.iter().all(|status| *status == GRPC_UNAVAILABLE),
        "every refusal must be UNAVAILABLE, which a wallet retries: got {statuses:?}"
    );
    assert!(
        statuses.len() < total,
        "every stream was refused: the budget served none of them, so it is not being returned"
    );
    (drivers, streams)
}

#[tokio::test]
async fn a_flood_cannot_buffer_past_the_budget_and_the_budget_returns() {
    let backend = spawn_counting_backend(Arc::new(AtomicUsize::new(0))).await;
    let shim = spawn_forward_only_shim(backend).await;

    let (drivers, streams) = flood_connections(shim).await;

    // Drop the connections with every held stream on them. Each handler's body
    // read then fails, and its share of the budget comes back.
    for task in drivers.into_iter().chain(streams) {
        task.abort();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The same flood again is served the same way: the first flood's share
    // came back. A leak would refuse all of it.
    let (drivers, streams) = flood_connections(shim).await;
    for task in drivers.into_iter().chain(streams) {
        task.abort();
    }
}

#[tokio::test]
async fn streams_that_send_nothing_hold_no_budget() {
    // The reason the budget counts bytes. Request slots would be filled by
    // streams that deliver nothing at all, at no cost to the sender, and every
    // honest send would be refused behind them.
    let backend = spawn_counting_backend(Arc::new(AtomicUsize::new(0))).await;
    let shim = spawn_forward_only_shim(backend).await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let (mut sender, driver) = connect(shim).await;
    let streams = flood(&mut sender, shim, STREAMS, Bytes::new(), &tx).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut honest = connect_h2(shim).await;
    let reply = send_tx_reply(&mut honest, shim, V6_IRONWOOD_ONLY).await;
    assert_eq!(
        reply.status, 0,
        "an honest send was refused while {STREAMS} empty streams were open"
    );

    driver.abort();
    for stream in streams {
        stream.abort();
    }
}
