# zero-indexer-proto

The lightwalletd wire types and the `CompactTxStreamer` client and server stubs.
The hub and the shim share this crate.

## Layout

| Path | Content |
|---|---|
| `proto/` | `compact_formats.proto` and `service.proto`, copied from [zcash/lightwallet-protocol](https://github.com/zcash/lightwallet-protocol) at tag `v0.5.0` (`ac7cee0`) |
| `src/` | The Rust code generated from `proto/`. It is committed. |
| `gen/` | The generator. It is a separate workspace with its own lockfile. |

The crate has no build script. A build of the hub or the shim compiles `src/` as
it is, and needs no `protoc`.

## Regenerate

Install `protoc`, then run:

```sh
cargo run --locked --manifest-path zeronym/proto/gen/Cargo.toml
```

The `proto` job in `zeronym-guards.yml` runs this command and fails when the
result differs from the committed files.

## Update the protocol

1. Copy the two `.proto` files from the new lightwallet-protocol tag into `proto/`.
2. Change the tag and commit in the table above and in `src/lib.rs`.
3. Regenerate, and commit `proto/` and `src/` together.
4. Re-baseline `EXPECTED_SHA256` for the hub and the shim.

## Origin

Before this crate, the hub and shim took the same code from `zaino-proto` 0.6.0.
The generated files here match that release, apart from the module path
(`crate::compact_formats`) and one line wrap. The `.proto` files are under the
MIT license of the lightwallet-protocol repository.
