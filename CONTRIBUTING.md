# Contributing to zero-indexer

Thank you for your help. Read on for how to ask a question, which checks to run, and what a pull request needs.

## Questions and bugs

Ask questions and report bugs in [GitHub issues](https://github.com/ShieldedLabs/zero-indexer/issues).

Keep vulnerability details out of issues, pull requests and discussions. Report them through the [security policy](./SECURITY.md).

## Checks

You need the stable Rust toolchain and `cargo-vet` 0.10.2, the version CI pins. The root checks also need `protoc` 36.2 and Node.js.

The hub and the shim are separate Rust workspaces, in `hub/` and `shim/`. CI runs these checks on each pull request. Run them in `hub/` and again in `shim/`:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo clippy --all-targets --locked --all-features -- -D warnings
cargo test --all-targets
cargo test --all-targets --features mixnet-driver
cargo vet --locked
```

Run these from the repository root:

```bash
sh guards.sh
cargo run --locked --manifest-path proto/gen/Cargo.toml && git diff --exit-code -- proto/src
CHECK_TIERS=simulation sh spec/quint/check.sh
```

- `guards.sh` checks the repository's own claims, so that fixed review findings stay fixed.
- The `proto/gen` command regenerates the gRPC code in `proto/src`. Change `proto/src` only through this command. Use `protoc` 36.2, the version CI pins, so that the output matches CI.
- `CHECK_TIERS=simulation sh spec/quint/check.sh` runs the quick simulation tiers of the Quint spec. It needs Node.js, because it runs Quint with `npx`. On first use, Quint downloads its Rust evaluator.
- The exhaustive TLC tier needs Java and about 16 GB of memory for a full local run. Quint downloads Apalache, which carries TLC, on first use. CI runs this tier in three shards, with `CHECK_TIERS=tlc`, `CHECK_SHARD=1/3` to `3/3`, `QUINT_JOBS=4`, `TLC_HEAP=3g` and `TLC_TIMEOUT=900`.

## Pull requests

- Keep one change in each pull request.
- A pull request into `main` needs one approval, all review threads resolved, and all `zero-indexer-guards` checks green.
- Pull requests are squash-merged. The title becomes the commit subject. Write it as a [conventional commit](https://www.conventionalcommits.org/): `type(scope): summary`. For example, `feat(hub): scale the schedule to NU7 blocks and the 120-block expiry`. The scope is optional. Scopes in recent titles include `hub`, `readme` and `releasing`.
- Release notes go under `## Unreleased` in the [changelog](./CHANGELOG.md), as described in the [release playbook](./RELEASING.md).

## Reproducible builds

`hub/deploy/EXPECTED_SHA256` and `shim/deploy/EXPECTED_SHA256` hold the hash of each binary as built from `main`. The reproduce workflows build each binary two times from cold and compare the result with that hash.

The binary's hash depends on `src/`, `Cargo.toml`, `Cargo.lock` and `proto/src/`. Any edit to them changes the hash, even one that only moves lines in `src/`. The other inputs are listed in the deploy READMEs for the [hub](./hub/deploy/README.md) and the [shim](./shim/deploy/README.md). Put a new hash in its own commit. Say so in the pull request. For the shim, follow the re-baseline steps in its [deploy README](./shim/deploy/README.md). Update the current row of its "Recorded hashes" table in the same commit. `guards.sh` fails if that row and `EXPECTED_SHA256` differ.

## License

Unless you state otherwise, your contribution is licensed under `MIT OR Apache-2.0`, as stated in the [License](./README.md#license) section of the README.
