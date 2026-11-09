//! A forwarded request body holds its budget charge until its bytes leave the
//! shim.
//!
//! The shim forwards a pass-through `SendTransaction` from its buffered body.
//! An upstream may answer at once and then keep its flow-control window shut
//! and the stream open. h2 then holds the unsent bytes until the window opens
//! or the stream or connection ends. A half-dead backend does this, and so can
//! the operator.
//!
//! The budget must count those bytes. The test sends full-size bodies to such
//! an upstream until the shim refuses one, which shows the charges are still
//! held. Then the upstream resets one stream, and later closes its connection,
//! and the charges come back.
//!
//! Found in review of the fix for an external report, @ouicate.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http2 as client_h2;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

mod common;
use common::{connect_h2, grpc_call, spawn_forward_only_shim};
use zero_indexer_shim::proxy::SEND_TRANSACTION;

/// Mirrors `intercept::MAX_SEND_TX_BYTES`: the most one body may buffer.
const BODY: usize = 4 * 1024 * 1024;

/// Mirrors `intercept::BODY_BUDGET_BYTES` / `BODY`: the full bodies it holds.
const BODIES_IN_BUDGET: usize = 64;

/// Well past the budget, so a shim that drops the charges is caught.
const MAX_SENDS: usize = BODIES_IN_BUDGET + 16;

/// Time for a reset or a closed connection to reach the shim.
const SETTLE: Duration = Duration::from_millis(500);

const GRPC_OK: i32 = 0;
const GRPC_UNAVAILABLE: i32 = 14;

enum Command {
    /// Reset the newest held stream.
    ResetOne,
    /// Drop the TCP connection, as a crashed upstream would.
    Close,
}

/// An upstream that answers every request with a trailers-only OK and leaves
/// the body unread. It keeps each request body and response stream, so h2
/// keeps the window shut and the stream open. It serves one connection at a
/// time, and counts the streams it accepts.
async fn spawn_unread_upstream() -> (SocketAddr, mpsc::UnboundedSender<Command>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (control, mut commands) = mpsc::unbounded_channel();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let mut conn = h2::server::handshake(socket).await.unwrap();
            let mut held = Vec::new();
            loop {
                tokio::select! {
                    accepted = conn.accept() => {
                        let Some(Ok((request, mut respond))) = accepted else { break };
                        count.fetch_add(1, Ordering::SeqCst);
                        let response = http::Response::builder()
                            .header("content-type", "application/grpc")
                            .header("grpc-status", "0")
                            .body(())
                            .unwrap();
                        let stream = respond.send_response(response, true).unwrap();
                        held.push((request.into_body(), stream));
                    }
                    command = commands.recv() => match command {
                        Some(Command::ResetOne) => {
                            let (_body, mut stream) = held.pop().expect("a held stream");
                            stream.send_reset(h2::Reason::CANCEL);
                        }
                        Some(Command::Close) | None => break,
                    },
                }
            }
            // `conn` drops here unpolled, so the socket closes as a bare TCP close.
        }
    });
    (addr, control, accepted)
}

async fn send(
    wallet: &mut client_h2::SendRequest<BoxBody<Bytes, Infallible>>,
    shim: SocketAddr,
    body: &Bytes,
) -> i32 {
    grpc_call(
        wallet,
        shim,
        SEND_TRANSACTION,
        Full::new(body.clone()).boxed(),
    )
    .await
    .status
}

/// Send full-size bodies until the shim refuses one. Returns how many it
/// forwarded first.
async fn fill(
    wallet: &mut client_h2::SendRequest<BoxBody<Bytes, Infallible>>,
    shim: SocketAddr,
    body: &Bytes,
) -> usize {
    for served in 0..MAX_SENDS {
        match send(wallet, shim, body).await {
            GRPC_OK => {}
            GRPC_UNAVAILABLE => return served,
            other => panic!("unexpected gRPC status {other} after {served} sends"),
        }
    }
    panic!(
        "the shim served all {MAX_SENDS} bodies of {BODY} bytes to an upstream that holds them \
         unread: their bytes sit outside the {BODIES_IN_BUDGET}-body budget"
    );
}

#[tokio::test]
async fn forwarded_bytes_hold_their_charge_until_the_stream_or_connection_ends() {
    let (upstream, control, accepted) = spawn_unread_upstream().await;
    let shim = spawn_forward_only_shim(upstream).await;
    let mut wallet = connect_h2(shim).await;
    // Zeros fail classification, and a shim with no hub forwards them.
    let body = Bytes::from(vec![0u8; BODY]);

    let served = fill(&mut wallet, shim, &body).await;
    assert!(
        served > 0 && served < BODIES_IN_BUDGET,
        "{served} unread bodies fit a budget of {BODIES_IN_BUDGET}"
    );
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        served,
        "the upstream accepted exactly the streams the shim served"
    );

    control.send(Command::ResetOne).unwrap();
    tokio::time::sleep(SETTLE).await;
    assert_eq!(
        send(&mut wallet, shim, &body).await,
        GRPC_OK,
        "a stream the upstream reset still holds its charge"
    );
    assert_eq!(
        send(&mut wallet, shim, &body).await,
        GRPC_UNAVAILABLE,
        "the body forwarded after the reset was served outside the budget"
    );

    control.send(Command::Close).unwrap();
    tokio::time::sleep(SETTLE).await;
    assert_eq!(
        fill(&mut wallet, shim, &body).await,
        served,
        "a closed upstream connection kept some of its charges held"
    );
}
