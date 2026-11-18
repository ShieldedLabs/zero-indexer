//! The pinned parser against the chain as it is today.
//!
//! At NU7 every transaction on the network stopped parsing here, because the
//! `zebra-chain` this crate pins predated the new consensus branch. Nothing
//! said so until the upgrade had activated. This reads the latest blocks from a
//! real lightwalletd, through a forward-only shim, and fails on any transaction
//! the classifier cannot parse, or parses to a different txid than the chain
//! gives it.
//!
//! It also fails if the server is on another chain than the one named, or if
//! its newest block is old. A node that was not upgraded stalls at the last
//! block it accepts, and every block before that still parses.
//!
//! Ignored by default: it needs a reachable `CompactTxStreamer`. The daily
//! `zeronym-latest-blocks` workflow runs it against mainnet and testnet. An
//! upgrade activates on testnet first, so that run is the early warning.
//!
//! The address is a literal and the name is what the certificate must carry,
//! as for the shim's own `--backend` and `--backend-tls`. The chain is `main`
//! or `test`, as the server's `GetLightdInfo` reports it:
//!
//!   ZIS_TEST_BACKEND=<ip>:443 ZIS_TEST_BACKEND_TLS=<name> ZIS_TEST_CHAIN=main \
//!     cargo test --test latest_blocks -- --ignored --nocapture

mod common;

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt, Full};
use prost::Message;
use tokio::net::TcpListener;
use zero_indexer_proto::compact_formats::CompactBlock;
use zero_indexer_proto::service::{BlockId, BlockRange, ChainSpec, Empty, LightdInfo, PoolType};

use zero_indexer_shim::classify::classify_with_evidence;
use zero_indexer_shim::proxy::Backend;
use zero_indexer_shim::tls::BackendTls;

use common::{connect_h2, decode_raw_transaction, get_transaction, grpc_call, grpc_frame};

const GET_LIGHTD_INFO: &str = "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLightdInfo";
const GET_LATEST_BLOCK: &str = "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetLatestBlock";
const GET_BLOCK_RANGE: &str = "/cash.z.wallet.sdk.rpc.CompactTxStreamer/GetBlockRange";

/// How many blocks back from the tip to read: about 17 minutes of chain at 25
/// seconds a block. An upgrade changes every transaction after it, so a short
/// window sees one as surely as a long one.
const BLOCKS: u64 = 40;

/// Blocks left alone at the tip. A block reorged away while this runs takes its
/// coinbase with it, and the lookup for that one would fail the run for nothing.
const BELOW_TIP: u64 = 5;

/// How old the newest block read may be. Blocks come every 25 seconds, so an
/// hour without one is a server that has stopped following the chain.
const MAX_TIP_AGE: Duration = Duration::from_secs(60 * 60);

/// A gap between lookups, so a public indexer is not asked for a few hundred
/// transactions in one burst.
// shortcut: one attempt per call, so an indexer that refuses, stalls or
// restarts mid-run fails the run; add a retry if the daily run flakes on that.
const PACE: Duration = Duration::from_millis(100);

/// Split a server-streaming reply into its gRPC messages.
fn messages(mut body: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while body.len() >= 5 {
        let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
        assert!(body.len() >= 5 + len, "a truncated gRPC message");
        out.push(&body[5..5 + len]);
        body = &body[5 + len..];
    }
    assert!(
        body.is_empty(),
        "trailing bytes after the last gRPC message"
    );
    out
}

#[tokio::test]
#[ignore = "needs a reachable lightwalletd; set ZIS_TEST_BACKEND"]
async fn every_transaction_in_the_latest_blocks_parses() {
    let addr: SocketAddr = std::env::var("ZIS_TEST_BACKEND")
        .expect("set ZIS_TEST_BACKEND=ip:port")
        .parse()
        .expect("a literal IPv4:port, as the shim takes its backend");
    let tls = std::env::var("ZIS_TEST_BACKEND_TLS")
        .ok()
        .map(|name| BackendTls::new(&name).expect("a valid DNS name"));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shim = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = zero_indexer_shim::serve_with_shutdown(
            listener,
            Backend { addr, tls },
            None,
            None,
            zero_indexer_shim::CautionRelay::default(),
            false,
            zero_indexer_shim::nym::MixnetStatus::default(),
            std::future::pending::<()>(),
        )
        .await;
    });
    let mut sender = connect_h2(shim).await;

    // The network is otherwise only a label on the run: a testnet job pointed
    // at a mainnet server would pass every day and warn of nothing.
    let chain = std::env::var("ZIS_TEST_CHAIN").expect("set ZIS_TEST_CHAIN=main or test");
    let reply = grpc_call(
        &mut sender,
        shim,
        GET_LIGHTD_INFO,
        Full::new(grpc_frame(&Empty {}.encode_to_vec())).boxed(),
    )
    .await;
    assert_eq!(reply.status, 0, "GetLightdInfo: {:?}", reply.message);
    let info = LightdInfo::decode(messages(&reply.body)[0]).expect("a LightdInfo");
    assert_eq!(info.chain_name, chain, "the server is on another chain");

    let reply = grpc_call(
        &mut sender,
        shim,
        GET_LATEST_BLOCK,
        Full::new(grpc_frame(&ChainSpec {}.encode_to_vec())).boxed(),
    )
    .await;
    assert_eq!(reply.status, 0, "GetLatestBlock: {:?}", reply.message);
    let last = BlockId::decode(messages(&reply.body)[0])
        .expect("a BlockId")
        .height
        - BELOW_TIP;

    // Every pool, so the transparent-only transactions and the coinbases come
    // back too. A range that names no pool gets shielded data only.
    let at = |height| {
        Some(BlockId {
            height,
            hash: Vec::new(),
        })
    };
    let range = BlockRange {
        start: at(last + 1 - BLOCKS),
        end: at(last),
        pool_types: vec![
            PoolType::Transparent as i32,
            PoolType::Sapling as i32,
            PoolType::Orchard as i32,
            PoolType::Ironwood as i32,
        ],
    };
    let reply = grpc_call(
        &mut sender,
        shim,
        GET_BLOCK_RANGE,
        Full::new(grpc_frame(&range.encode_to_vec())).boxed(),
    )
    .await;
    assert_eq!(reply.status, 0, "GetBlockRange: {:?}", reply.message);
    let blocks: Vec<CompactBlock> = messages(&reply.body)
        .into_iter()
        .map(|message| CompactBlock::decode(message).expect("a CompactBlock"))
        .collect();
    // Exactly the heights asked for, once each and in the rising order the
    // protocol returns them in. A count alone would pass a server that
    // answered with one block forty times.
    let heights: Vec<u64> = blocks.iter().map(|block| block.height).collect();
    let wanted: Vec<u64> = (last + 1 - BLOCKS..=last).collect();
    assert_eq!(
        heights, wanted,
        "the server returned other blocks than the range asked for"
    );

    // Old blocks always parse, so without this a server stalled at an upgrade
    // would pass on exactly the day this exists to fail.
    let newest = blocks.iter().map(|block| block.time).max().unwrap_or(0);
    let age = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock past 1970")
        .as_secs()
        .saturating_sub(u64::from(newest));
    assert!(
        age <= MAX_TIP_AGE.as_secs(),
        "the newest block read is {age} s old: the server is not following the chain"
    );

    let mut read = 0;
    let mut failures = Vec::new();
    for block in &blocks {
        // The coinbase at least. Without this an indexer that returned empty
        // blocks would pass with nothing parsed.
        assert!(
            !block.vtx.is_empty(),
            "block {} came back with no transactions",
            block.height
        );
        for tx in &block.vtx {
            tokio::time::sleep(PACE).await;
            let reply = get_transaction(&mut sender, shim, &tx.txid).await;
            assert_eq!(reply.status, 0, "GetTransaction: {:?}", reply.message);
            let raw = decode_raw_transaction(&reply.body).data;
            read += 1;

            let mut txid = tx.txid.clone();
            txid.reverse();
            let txid = hex::encode(txid);
            let failure = match classify_with_evidence(&raw).error {
                Some(error) => Some(error),
                None if common::wire_hash(&raw) != tx.txid => {
                    Some("parsed to a different txid".to_owned())
                }
                None => None,
            };
            if let Some(why) = failure {
                // Printed now as well as in the summary: a later lookup that
                // fails would end the run before the summary is reached.
                eprintln!("block {} tx {txid}: {why}", block.height);
                failures.push(format!("block {} tx {txid}: {why}", block.height));
            }
        }
    }

    println!(
        "blocks {}..={last}: {read} transactions, {} failed",
        last + 1 - BLOCKS,
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {read} transactions failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
