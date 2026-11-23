# Contributing to zero-indexer

Thank you for your help. This file tells you how to ask a question, which checks to run, and what a pull request needs.

## Questions and bugs

Ask questions and report bugs in [GitHub issues](https://github.com/ShieldedLabs/zero-indexer/issues).

Do not report a vulnerability in an issue, a pull request or a discussion. Follow the [security policy](./SECURITY.md).

## Checks

You need the stable Rust toolchain and `cargo-vet` 0.10.2, the version CI pins. The root checks also need `protoc` 36.2, Node.js and Java.

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
sh spec/quint/check.sh
```

- `guards.sh` checks claims that the repository makes about itself.
- The `proto/gen` command regenerates the gRPC code in `proto/src`. Do not edit `proto/src` by hand. Use `protoc` 36.2: another version can give a different result.
- `spec/quint/check.sh` runs Quint with `npx`, and its last tier runs TLC, which needs Java. On first use it downloads Quint's Rust evaluator and Apalache.

## Pull requests

- Keep one change in each pull request.
- A pull request into `main` needs one approval, all review threads resolved, and all `zeronym-guards` checks green.
- Pull requests are squash-merged. The title becomes the commit subject, so write the title as a [conventional commit](https://www.conventionalcommits.org/): `type(scope): summary`. For example, `feat(hub): scale the schedule to NU7 blocks and the 120-block expiry`. The scope is optional. Recent titles use `hub` and `readme`.
- Release notes collect under `## Unreleased` in the [changelog](./CHANGELOG.md), as the [release playbook](./RELEASING.md) describes.

## Reproducible builds

`hub/deploy/EXPECTED_SHA256` and `shim/deploy/EXPECTED_SHA256` hold the hash of each binary as built from `main`. The reproduce workflows build each binary two times from cold and compare the result with that hash.

Any change to `src/`, `Cargo.toml`, `Cargo.lock` or `proto/src/` changes the binary's hash, even a change that only moves lines in `src/`. The deploy READMEs for the [hub](./hub/deploy/README.md) and the [shim](./shim/deploy/README.md) list the other inputs. Put a new hash in its own commit, and say so in the pull request. For the shim, follow the re-baseline steps in its [deploy README](./shim/deploy/README.md): the same commit updates the current row of its "Recorded hashes" table, and `guards.sh` fails if the two differ.

## License

Unless you state otherwise, your contribution is licensed as the [License](./README.md#license) section of the README says: `MIT OR Apache-2.0`.
