//! The lightwalletd wire types and the `CompactTxStreamer` client and server
//! stubs.
//!
//! `proto/` holds the two files from zcash/lightwallet-protocol at v0.5.0
//! (`ac7cee0`).
//! `gen/` writes the two modules below from them. See `README.md`.

#![forbid(unsafe_code)]

#[allow(clippy::all)]
#[rustfmt::skip]
pub mod compact_formats;
#[allow(clippy::all)]
#[rustfmt::skip]
pub mod service;
