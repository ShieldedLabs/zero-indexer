//! A wallet that never reads its responses must not pin unbounded indexer data
//! in the shim.
//!
//! The fake indexer answers every call with an endless body and counts the
//! bytes the shim pulls from it. The wallet opens streams and never reads them.
//! Once the count stops moving, it is what the shim holds for that wallet.
//!
//! Two ways to never read, and each fills a different buffer:
//!
//! * A wallet that grants no HTTP/2 window fills the indexer leg's window.
//!   Each wallet connection has one indexer connection, so the connection
//!   window bounds it.
//! * A wallet that grants a large window and stops reading its socket fills
//!   hyper's send buffer on every stream.
//!
//! Both bounds also count one 16 KiB chunk per stream in the shim and one in
//! the fake indexer: hyper takes the next chunk from a body before it waits for
//! window to send it. That part grows with the streams on a connection, so
//! the wallet asks for hyper's default of 200 and the shim's stream cap must
//! hold it to 32.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{Empty, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::client::conn::http2 as client_h2;
use hyper::server::conn::http2 as server_h2;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

const CHUNK: usize = 16 * 1024;
static ZEROS: [u8; CHUNK] = [0; CHUNK];
const MIB: usize = 1024 * 1024;

/// `UPSTREAM_CONNECTION_WINDOW` in `src/proxy.rs`.
const UPSTREAM_CONNECTION_WINDOW: usize = MIB;

/// `MAX_CONCURRENT_STREAMS` in `src/proxy.rs`.
const MAX_CONCURRENT_STREAMS: usize = 32;

/// The streams the wallet asks for on each connection: hyper's default cap.
const REQUESTED_STREAMS: usize = 200;

/// Room for the kernel's socket buffers on the wallet's connection, which the
/// socket-stall test fills too. Linux autotunes a send buffer to 4 MiB.
const KERNEL_SLACK: usize = 8 * MIB;

const GET_BLOCK_RANGE: &str = "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetBlockRange";

#[derive(Default)]
struct Counts {
    /// Calls that reached the indexer.
    requests: AtomicUsize,
    /// Bytes the shim has pulled from the indexer.
    produced: AtomicUsize,
}

/// An indexer whose every response body is endless.
async fn spawn_firehose_backend(counts: Arc<Counts>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let counts = counts.clone();
            tokio::spawn(async move {
                let _ = server_h2::Builder::new(TokioExecutor::new())
                    .serve_connection(
                        TokioIo::new(stream),
                        service_fn(move |_req: Request<Incoming>| {
                            let counts = counts.clone();
                            counts.requests.fetch_add(1, Ordering::Relaxed);
                            async move {
                                let chunks = futures_util::stream::repeat_with(move || {
                                    counts.produced.fetch_add(CHUNK, Ordering::Relaxed);
                                    Ok::<_, Infallible>(Frame::data(Bytes::from_static(&ZEROS)))
                                });
                                let mut resp = Response::new(StreamBody::new(chunks));
                                resp.headers_mut()
                                    .insert("content-type", "application/grpc".parse().unwrap());
                                Ok::<_, Infallible>(resp)
                            }
                        }),
                    )
                    .await;
            });
        }
    });
    addr
}

async fn spawn_shim(backend: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = zero_indexer_shim::serve_with_shutdown(
            listener,
            backend,
            None,
            None,
            zero_indexer_shim::CautionRelay::default(),
            false,
            zero_indexer_shim::nym::MixnetStatus::default(),
            std::future::pending::<()>(),
        )
        .await;
    });
    addr
}

/// A socket the wallet can stop reading from, while it keeps writing.
struct StallableIo {
    inner: TcpStream,
    stalled: Arc<AtomicBool>,
}

impl AsyncRead for StallableIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.stalled.load(Ordering::Relaxed) {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for StallableIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

type Pending = Pin<Box<dyn std::future::Future<Output = hyper::Result<Response<Incoming>>> + Send>>;

/// Open `streams` GetBlockRange calls on one wallet connection and never poll
/// them, so no response body is ever read. Returns what must stay alive.
async fn open_unread_streams(
    shim: SocketAddr,
    streams: usize,
    window: u32,
    stalled: bool,
) -> (
    client_h2::SendRequest<BoxBody<Bytes, Infallible>>,
    Vec<Pending>,
) {
    let io = StallableIo {
        inner: TcpStream::connect(shim).await.unwrap(),
        stalled: Arc::new(AtomicBool::new(stalled)),
    };
    let (mut sender, conn) = client_h2::Builder::new(TokioExecutor::new())
        .initial_stream_window_size(window)
        .initial_connection_window_size(window.max(65_535))
        .handshake::<_, BoxBody<Bytes, Infallible>>(TokioIo::new(io))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut held: Vec<Pending> = Vec::new();
    for _ in 0..streams {
        let request = Request::builder()
            .method("POST")
            .uri(format!("http://{shim}{GET_BLOCK_RANGE}"))
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .body(BoxBody::new(Empty::<Bytes>::new()))
            .unwrap();
        // hyper's HTTP/2 `ready()` waits for no stream slot: a request over the
        // shim's cap queues in the client.
        sender.ready().await.unwrap();
        held.push(Box::pin(sender.send_request(request)));
    }
    (sender, held)
}

/// Wait until `produced` stops moving, then return it.
async fn settle(produced: &AtomicUsize) -> usize {
    let mut last = usize::MAX;
    let mut quiet = 0;
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let now = produced.load(Ordering::Relaxed);
        if now == last {
            quiet += 1;
            if quiet == 10 {
                return now;
            }
        } else {
            quiet = 0;
            last = now;
        }
    }
    panic!("the indexer never stopped producing: {} MiB", last / MIB);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wallet_that_grants_no_window_pins_one_upstream_window_per_connection() {
    let counts = Arc::new(Counts::default());
    let backend = spawn_firehose_backend(counts.clone()).await;
    let shim = spawn_shim(backend).await;

    // A stream window of zero: the shim can forward nothing to this wallet, so
    // every byte pulled from the indexer stays inside the shim. A second
    // connection shows the bound is per connection.
    let mut held = Vec::new();
    for connections in 1..=2 {
        held.push(open_unread_streams(shim, REQUESTED_STREAMS, 0, false).await);
        let pinned = settle(&counts.produced).await;
        // The floor shows the indexer window fills, so the test measures it.
        let floor = connections * UPSTREAM_CONNECTION_WINDOW;
        let bound = connections * (UPSTREAM_CONNECTION_WINDOW + MAX_CONCURRENT_STREAMS * 2 * CHUNK);
        assert!(
            (floor..=bound).contains(&pinned),
            "{connections} connection(s) pinned {pinned} bytes, outside {floor}..={bound}"
        );
        assert_eq!(
            counts.requests.load(Ordering::Relaxed),
            connections * MAX_CONCURRENT_STREAMS,
            "{connections} connection(s) asking for {REQUESTED_STREAMS} streams each"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wallet_that_stops_reading_its_socket_pins_a_frame_per_stream() {
    let counts = Arc::new(Counts::default());
    let backend = spawn_firehose_backend(counts.clone()).await;
    let shim = spawn_shim(backend).await;

    // The largest window HTTP/2 allows, and a socket that is never read.
    // hyper's default send buffer let this pin 400 KiB per stream. The stall
    // starts before the handshake, which only writes: a wallet that read even
    // once would pull response data into its own window. It never sees the
    // shim's SETTINGS, so it sends all 200 HEADERS and the shim refuses the
    // streams over its cap.
    let _held = open_unread_streams(shim, REQUESTED_STREAMS, i32::MAX as u32, true).await;
    let pinned = settle(&counts.produced).await;
    // The floor shows every served stream pulled at least one frame.
    let floor = counts.requests.load(Ordering::Relaxed) * CHUNK;
    let bound = UPSTREAM_CONNECTION_WINDOW + MAX_CONCURRENT_STREAMS * 3 * CHUNK + KERNEL_SLACK;
    assert!(
        floor > 0 && (floor..=bound).contains(&pinned),
        "pinned {pinned} bytes, outside {floor}..={bound}"
    );
}
