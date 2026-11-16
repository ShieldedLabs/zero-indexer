//! `/healthz` on a shim whose build does not know the chain's consensus branch.
//!
//! A shim built before a network upgrade reads every transaction on the new
//! consensus branch as unparseable. With a hub it diverts all of them, and
//! answers every lookup of a mined one with NOT_FOUND. It says so in a log,
//! and the operator of an attested shim cannot read that log. So the status
//! code has to say it.
//!
//! One test, in a file of its own: what it watches is set once per process,
//! and a second test in this binary could set it first.

mod common;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1 as server_h1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use zero_indexer_shim::hub::HubClient;
use zero_indexer_shim::intercept::Diversion;
use zero_indexer_shim::proxy::SHIM_HEALTH;

use common::{bounded, connect_h2, get_transaction, send_tx, spawn_counting_backend, V6_MIGRATION};

/// A V5 header naming a consensus branch that does not exist. This is what
/// every transaction on the network looks like to a shim built before the
/// upgrade that introduced its branch.
fn v5_on_an_unknown_branch() -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(&0x8000_0005u32.to_le_bytes()); // fOverwintered | v5
    raw.extend_from_slice(&0x26A7_270Au32.to_le_bytes()); // nVersionGroupId (v5)
    raw.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes()); // nConsensusBranchId
    raw.extend_from_slice(&[0u8; 16]); // lock_time, expiry_height, empty bundles
    raw
}

/// A hub that accepts every submission and answers every lookup with `data` at
/// a mined height, which is how the real hub answers for a transaction its
/// node has. A queued one comes back with no bytes at all.
async fn spawn_hub_serving(data: Vec<u8>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let data = data.clone();
            tokio::spawn(async move {
                let answer = service_fn(move |req: Request<Incoming>| {
                    let data = data.clone();
                    async move {
                        let reply = if req.uri().path() == "/transaction" {
                            Response::builder()
                                .header("content-type", "application/octet-stream")
                                .header("x-tx-height", "3600000")
                                .body(Full::new(Bytes::from(data)))
                                .unwrap()
                        } else {
                            Response::new(Full::new(Bytes::from_static(
                                b"{\"disposition\":\"accepted\",\"txid\":\"00\"}",
                            )))
                        };
                        Ok::<_, Infallible>(reply)
                    }
                });
                let _ = server_h1::Builder::new()
                    .serve_connection(TokioIo::new(stream), answer)
                    .await;
            });
        }
    });
    addr
}

async fn spawn_diverting_shim(backend: SocketAddr, hub: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let diversion = Some(Arc::new(Diversion {
        hub: HubClient::new(hub, None).into(),
    }));
    tokio::spawn(async move {
        let _ = zero_indexer_shim::serve_with_shutdown(
            listener,
            backend,
            None,
            diversion,
            zero_indexer_shim::CautionRelay::default(),
            false,
            zero_indexer_shim::nym::MixnetStatus::default(),
            std::future::pending::<()>(),
        )
        .await;
    });
    addr
}

async fn healthz(shim: SocketAddr) -> (StatusCode, String) {
    let mut sender = connect_h2(shim).await;
    let request = Request::builder()
        .method("GET")
        .uri(format!("http://{shim}{SHIM_HEALTH}"))
        .body(Full::new(Bytes::new()).boxed())
        .unwrap();
    sender.ready().await.unwrap();
    let response = bounded(sender.send_request(request)).await.unwrap();
    let status = response.status();
    let body = bounded(response.into_body().collect()).await.unwrap();
    (
        status,
        String::from_utf8_lossy(&body.to_bytes()).into_owned(),
    )
}

#[tokio::test]
async fn a_lookup_reply_on_an_unknown_branch_turns_healthz_to_503() {
    let backend = spawn_counting_backend(Arc::new(AtomicUsize::new(0))).await;
    let unknown_branch = v5_on_an_unknown_branch();

    // A hub answering a lookup with some other transaction is a hub at fault,
    // not an upgrade. The shim refuses the reply and stays healthy.
    let wrong = spawn_hub_serving(V6_MIGRATION.to_vec()).await;
    let shim = spawn_diverting_shim(backend, wrong).await;
    let mut sender = connect_h2(shim).await;
    assert_eq!(
        get_transaction(&mut sender, shim, &[0x11; 32]).await.status,
        5
    );
    assert_eq!(healthz(shim).await, (StatusCode::OK, "ok".to_owned()));

    let hub = spawn_hub_serving(unknown_branch.clone()).await;
    let shim = spawn_diverting_shim(backend, hub).await;
    let mut sender = connect_h2(shim).await;

    // A wallet chooses the bytes it sends, so nothing it sends may move this:
    // not junk, and not the very transaction that moves it below. Otherwise
    // one request from anyone turns the shim red until it is restarted.
    send_tx(&mut sender, shim, &[0xff; 64]).await;
    send_tx(&mut sender, shim, &unknown_branch).await;
    assert_eq!(healthz(shim).await, (StatusCode::OK, "ok".to_owned()));

    // The same bytes in a lookup reply come from one of the hub's indexers,
    // not from a wallet.
    assert_eq!(
        get_transaction(&mut sender, shim, &[0x11; 32]).await.status,
        5
    );
    let (status, body) = healthz(shim).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!body.is_empty(), "the reason must be stated, not implied");
}
