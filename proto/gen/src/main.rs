//! Writes `../src/compact_formats.rs` and `../src/service.rs` from `../proto/`.
//! Needs `protoc` on the PATH. Run from any directory:
//!
//!     cargo run --manifest-path zeronym/proto/gen/Cargo.toml

use std::fs;
use std::path::Path;

/// Both files declare this package, so each compile writes this file name.
const GENERATED: &str = "cash.z.wallet.sdk.rpc.rs";

/// The compact-format types `service.proto` imports. The service module
/// refers to the ones in `compact_formats` and holds no second copy.
const COMPACT_FORMAT_TYPES: [&str; 6] = [
    "ChainMetadata",
    "CompactBlock",
    "CompactTx",
    "CompactSaplingSpend",
    "CompactSaplingOutput",
    "CompactOrchardAction",
];

fn main() -> std::io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let proto = root.join("proto");
    let out = root.join("gen/target/generated");
    fs::create_dir_all(&out)?;

    tonic_prost_build::configure()
        .build_client(false)
        .build_server(false)
        .out_dir(&out)
        .compile_protos(&[proto.join("compact_formats.proto")], &[proto.clone()])?;
    fs::copy(out.join(GENERATED), root.join("src/compact_formats.rs"))?;

    COMPACT_FORMAT_TYPES
        .iter()
        .fold(
            tonic_prost_build::configure()
                .build_server(true)
                .out_dir(&out)
                // `Bytes` for the raw transaction: the shim and hub pass the
                // payload on by reference count.
                .bytes(".cash.z.wallet.sdk.rpc.RawTransaction.data"),
            |builder, name| {
                builder.extern_path(
                    format!(".cash.z.wallet.sdk.rpc.{name}"),
                    format!("crate::compact_formats::{name}"),
                )
            },
        )
        .compile_protos(&[proto.join("service.proto")], &[proto])?;
    fs::copy(out.join(GENERATED), root.join("src/service.rs"))?;
    Ok(())
}
