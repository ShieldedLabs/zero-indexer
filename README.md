# zero-indexer

[![checks](https://img.shields.io/github/actions/workflow/status/ShieldedLabs/zero-indexer/zeronym-guards.yml?branch=main&label=checks)](https://github.com/ShieldedLabs/zero-indexer/actions/workflows/zeronym-guards.yml)
[![hub reproducible build](https://img.shields.io/github/actions/workflow/status/ShieldedLabs/zero-indexer/zeronym-hub-reproduce.yml?branch=main&label=hub%20reproducible%20build)](https://github.com/ShieldedLabs/zero-indexer/actions/workflows/zeronym-hub-reproduce.yml)
[![shim reproducible build](https://img.shields.io/github/actions/workflow/status/ShieldedLabs/zero-indexer/zeronym-shim-reproduce.yml?branch=main&label=shim%20reproducible%20build)](https://github.com/ShieldedLabs/zero-indexer/actions/workflows/zeronym-shim-reproduce.yml)
[![latest blocks](https://img.shields.io/github/actions/workflow/status/ShieldedLabs/zero-indexer/zeronym-latest-blocks.yml?branch=main&label=latest%20blocks)](https://github.com/ShieldedLabs/zero-indexer/actions/workflows/zeronym-latest-blocks.yml)
[![release](https://img.shields.io/github/v/release/ShieldedLabs/zero-indexer)](https://github.com/ShieldedLabs/zero-indexer/releases/latest)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![made by Shielded Labs](https://img.shields.io/badge/made%20by-Shielded%20Labs-black)](https://shieldedlabs.net)

An attested front end for Zcash light-wallet indexers.

`zero-indexer` keeps a wallet's IP address away from the peer-to-peer network when the wallet spends Orchard funds. [Shielded Labs](https://shieldedlabs.net) builds and maintains it.

A light wallet sends its transactions to an indexer over [ZIP 307](https://zips.z.cash/zip-0307). The indexer operator sees the wallet's IP address and the time of each request. With `zero-indexer`, an attested shim in front of the indexer sends Orchard transactions to a hub, and the hub publishes them in batches. Against the operator and the wallet's ISP, a wallet hides among the matching transactions in its batch. Today a batch holds one transaction, so that protection is weak (see [Security](#security)).

The long-term goal is a private indexer for all wallet traffic, over Nym and inside an attested enclave, with private information retrieval (PIR) added later.

## Table of Contents

- [Security](#security)
- [Background](#background)
- [Install](#install)
- [Usage](#usage)
- [How it works](#how-it-works)
- [Status](#status)
- [Maintainers](#maintainers)
- [Thanks](#thanks)
- [Contributing](#contributing)
- [License](#license)

## Security

The adversaries are the server-side and network-metadata adversaries in Taylor Hornby's [wallet app threat model](https://zcash.readthedocs.io/en/latest/rtd_pages/wallet_threat_model.html). Wallet-local threats belong to the wallet in that model, and are out of scope here. To report a vulnerability, follow [`SECURITY.md`](./SECURITY.md).

The enclaves run on AWS Nitro through [Caution](https://caution.co). The *parent host* is the AWS instance that runs an enclave. Caution runs it for a managed app. In bring-your-own-cloud (BYOC) mode, which we advise against, the operator runs it. Every claim below assumes a shim with a hub configured (`ZIS_REQUIRE_DIVERSION=true`, auditor step 4).

**Protected**

- **Broadcast contents.** The wallet's TLS key is generated and kept inside the enclave, in Caution's Caddy server. The parent host carries a diverted transaction as ciphertext. The certificate is bound to the enclave in the attestation, and an auditor can check that binding on the auditor's own connection. A wallet checks only the public CAs. The operator controls the DNS zone, so it can get a second valid certificate and serve it from a position on a wallet's path. Certificate Transparency logs publish that certificate, among the routine ones each enclave restart orders.
- **Source IP, against the hub and the peer-to-peer network.** The hub publishes the transaction, so the network sees the hub's node as the origin. In clearnet mode the hub sees only the shim's IP address, and in Nym mode the mixnet hides even that. Both hold at any volume.
- **`GetTransaction`.** The shim sends every lookup to the hub, so the operator cannot watch a wallet fetch the transaction it just diverted. The lookup is still visible at the hub. On a queue miss the hub asks its own indexers, so this holds only while a different operator runs them.
- **The second leg of a TEX payment.** A payment from shielded funds to a [ZIP 320](https://zips.z.cash/zip-0320) (TEX) address takes two transactions, and the second names the first. The shim diverts every transparent-only spend, so when the first leg is a migration, both legs go to the hub. The cost is latency: every transparent-only spend waits for a batch.

**Not protected**

- **Which transaction a wallet's IP sent, against the operator and the wallet's ISP, at today's volume.** At today's batch size of one, a flush holds one wallet's transaction alone. The parent host and the ISP can link the wallet's IP address to it, and the operator can link the wallet to it.
  - The parent host sees each connection's source IP, and the size and time of each upload. On the clearnet hop, the parent host also sees the shim's 64 KiB upload to the hub, a second marker of the same event. Nym's cover traffic removes that second marker.
  - The operator sees the queries that the enclave forwards to its indexer. From the missing `SendTransaction`, it learns that a wallet diverted a transaction at time T. The operator identifies the wallet by its transparent addresses (next item). To learn the IP address too, the operator must interpose on the wallet's path. An auditor can detect that in step 5.
  - Anyone can identify a flush on chain (`hub/REVIEW.md`, "The batch is publicly identifiable"). In a larger flush, the anonymity set is at most the transactions that match in size, shape, anchor and expiry, less any the operator submitted.
- **Transparent-pool queries.** The shim forwards `GetTaddressTxids`, `GetTaddressBalance` and `GetAddressUtxos` to the operator. Through these calls, the reference light-client SDK sends the operator the wallet's full transparent address set. From that set, the operator can recover a diverted transaction's txid **and its value**. The same applies to a shielded transaction that spends a diverted transaction's transparent output, for example auto-shielding after a deshield.
- **The size of a wallet's submission.** TLS hides the content and shows the length. The wallet-to-shim leg has no padding, so the parent host and the ISP see `|tx|` and its arrival time. Sizes are public on chain, so a transaction of unusual size is easy to find in its batch. Only the wallet can pad this leg. The shim-to-hub leg has a fixed frame size.
- **Whether a txid is in a hub's queue.** A hub answers a lookup for a queued txid with "found, height 0", to anyone. A Nym hub answers on the mixnet only. A clearnet hub also answers on `POST /transaction`, where its shims send lookups.
- **The hub hop on clearnet.** A clearnet shim dials the hub address in its measured config, and checks only the name on the hub's certificate. A party on that path with a certificate for the name can read every diverted transaction. That position belongs to Caution or AWS on a managed app, and to the parent-host operator at either end in BYOC. Whoever controls the hub's DNS zone can get the certificate. An auditor ties the address to the attested hub in step 4, but only at audit time. In Nym mode, the Nym client encrypts each frame to a key that the hub's enclave generated.
- **Physical security and the platform are delegated to AWS and Caution.** Nitro keeps the operator out of the enclave, so the guarantees above hold against everyone except AWS. Caution runs the parent host of a managed app and the Caddy server that ends the wallet's TLS. Caddy is outside `EXPECTED_SHA256` and outside this repository's review. Which PCR measures it is an open question for Caution.

## Background

A light wallet reads the chain through an indexer. Trial decryption happens on the wallet, so the indexer never learns which notes are the wallet's. But the indexer ends the wallet's connection, so it sees the source IP, the time of each request, and the addresses the wallet queries.

![Current Zcash transaction publication: wallets connect over TLS directly to one of a handful of indexers, each publishing to the mempool](./images/zcash_current_publication.svg)

A transaction that crosses a value-pool boundary shows the movement on chain. So the operator holds both halves of a link: *IP X broadcast at time T*, and *a pool-crossing transaction moving Y appeared at time T*. With both, the operator can link **IP address to on-chain transaction to balance**. The leak is in the transport, where the shielded cryptography has no reach. The chain is permanent, so the link also works after the fact.

The acute case is the migration from Orchard to Ironwood. Zooko called it the worst privacy-loss event in Zcash history: it is **mandatory**, **mass**, and **concentrated** in a bounded window. A large, non-urgent set of transactions can be batched, so migrations are where this project starts. Here, a *migration* is any transaction with Orchard actions.

## Install

Indexer operators deploy `zero-indexer`. The shim runs behind the operator's existing public URL and in front of the existing, unmodified indexer. Wallets need no change.

Follow the operator runbook, [`shim/deploy/caution/OPERATORS.md`](./shim/deploy/caution/OPERATORS.md). It covers prerequisites, deployment, verification and the config reference. A third-party operator has completed it end to end. First create an empty public git repository for the app source. Then assemble the shim's enclave app:

```bash
git clone https://github.com/ShieldedLabs/zero-indexer && cd zero-indexer
sh shim/deploy/caution/assemble-caution.sh \
  --name        <enclave-name> \
  --backend     <indexer-ipv4>:<port> \
  --backend-tls <name-on-indexer-cert> \
  --tls-domain  <wallet-facing-domain> \
  --app-source  <public-git-url> \
  --hub         <hub-ip>:443 \
  --hub-tls     <hub-domain>
```

Without `--hub` or `--hub-nym`, the shim forwards every transaction and gives no privacy. Shielded Labs gives you the hub's address. The runbook also covers the Nym transport.

### Dependencies

- A [Caution](https://caution.co) account and the `caution` CLI. An account needs an access code. The runbook tells how to get both.
- An empty public git repository, for the app source that auditors rebuild.
- Your indexer, lightwalletd or Zaino, at a literal `IPv4:port` over TLS.
- A DNS name that you control, for wallets.

## Usage

- **Wallet users** change nothing.
- **Wallet developers** must choose **aligned anchors and expiry heights** within a migration epoch, as in [ZIP 318](https://zips.z.cash/zip-0318). A transaction with the latest anchor is dated by that anchor, and an observer can use the date to find it in the published batch. Also plan for these behaviors:
  - The hub holds a diverted transaction for up to about 8 minutes.
  - Over Nym, the shim reports success before the hub confirms receipt, so a migration can fail silently. Anyone can cause this by filling a hub queue lane ([hub failure modes](./hub/deploy/caution/OPERATORS.md#known-failure-modes)). Over clearnet, the shim waits for the hub and passes a refusal to the wallet.
  - The wallet must resend a migration that does not confirm, because its notes stay locked until expiry, 30 to 60 days later. Resends are safe: the hub deduplicates them, and answers `rejected` for a transaction that is already mined.
- **Operators** run the shim, and can also run a hub. With a hub configured, migrations, transparent-only spends and `GetTransaction` lookups leave the operator's view. So does a `SendTransaction` that the shim cannot read. All other traffic passes through. With no hub, the shim gives no privacy: it forwards every transaction it can read.
- **Auditors** verify an endpoint with no trust in its operator. This is the full procedure, and each component guide links here. Caddy serves `/attestation` and `/nym-address` and is outside `EXPECTED_SHA256`, so steps 2 and 4 trust it. Do all five steps on the shim, then on each hub that the shim lists:
  1. **Reproduce the build.** Run `<component>/deploy/reproduce.sh` at the commit that the endpoint's `PROVENANCE` names. The binary hash must match `EXPECTED_SHA256`.
  2. **Verify the attestation.** In a fresh clone of the public app-source repo, run `caution verify --attestation-url https://<domain>/attestation`. It rebuilds the EIF, compares PCR0, PCR1 and PCR2 with the live attestation, and checks the TLS certificate binding. Require **all three PCRs**. (PCR0 and PCR1 are one code value within an enclave. PCR2 is the platform, the same for hub and shim.)
  3. **Re-assemble and diff.** `caution verify` rebuilds from the repository the operator named, so a PASS means only that the enclave runs the operator's tree. Run `assemble-caution.sh` from `github.com/ShieldedLabs/zero-indexer` at the `PROVENANCE` commit, with the parameters in the published `caution.hcl`. Then `diff -r` the result against the clone from step 2. Each difference is a finding.
  4. **Check each parameter** in the published `caution.hcl`. The operator chose them.
     - Diversion: `ZIS_REQUIRE_DIVERSION=true`, and exactly one hub transport after blank `ZIS_HUB_NYM` entries are dropped: a `ZIS_HUB` socket address, or one or more Nym addresses.
     - Clearnet hub: `ZIS_HUB` is the address the hub's domain resolves to through `<app-id>.apps.caution.sh`, and `ZIS_HUB_TLS` is that domain.
     - Nym hub: each `ZIS_HUB_NYM` entry equals the `/nym-address` of a hub you verified.
     - Hub indexers: the operator of this shim's `ZIS_BACKEND` is different from the operator of each `ZIH_INDEXERS` endpoint and `ZIH_INDEXER_TLS` name, on every hub the shim lists. Evidence of operator identity is out of band.
     - `ZIS_DIAG` is unset, and `debug.enabled = false`.
  5. **Resolve the domain** with `dig`, and confirm that it points at the Caution-managed target for the app id in the published artifacts. All the other checks pass through a plain TCP forwarder, so a layer-4 interposition is invisible to them. With this step, an auditor can see it. In BYOC the operator runs that target, so this step is weaker.

To run a shim and a hub locally on the public Nym mixnet, with no deploy, build both with the mixnet driver and run the smoke test. It needs outbound internet access, `python3` and a `curl` with HTTP/2. On mainnet the hub warms up for at least five minutes before the checks run:

```bash
(cd shim && cargo build --release --features mixnet-driver)
(cd hub && cargo build --release --features mixnet-driver)
sh smoke-local.sh
```

### CLI

The shim and the hub take their options as CLI flags or environment variables. The config reference in each runbook lists the main options: [shim](./shim/deploy/caution/OPERATORS.md#config-reference), [hub](./hub/deploy/caution/OPERATORS.md#config-reference). `--help` lists every flag, with its environment variable:

```bash
cd shim && cargo run --locked -- --help
cd ../hub && cargo run --locked -- --help
```

## How it works

![Zero-indexer transaction publication: wallets reach a shim inside a TEE at each organization, which routes Orchard-touching transactions over the Nym mixnet to a hub in its own TEE, which publishes to the mempool](./images/zcash_zero_indexer_publication.svg)

Green boxes are attested enclaves, the only places a migration is in cleartext. The grey edge to each indexer is the **pass-through path**, as before. The green edge is the **diverted path**. Each shim runs its Nym client in-process, and wallets never use the mixnet. Migrations from both organizations leave the mixnet as one stream, so the hub cannot tell which shim sent which.

- **`zero-indexer-shim`** is an attested router behind the operator's existing URL. It routes each `SendTransaction` by the pools it touches, and ignores amounts and destinations:

  ```text
  divert(tx) := is_orchard_touching(tx) || is_transparent_only_spend(tx)
  ```

  Orchard has taken no new value since NU6.3. So any Orchard spend is from notes held since before activation, and the spend itself is the identifying event. The shim classifies before it dials the indexer. With a hub configured, it diverts or refuses a transaction it cannot parse, and never forwards it. It keeps no per-migration state.
- **`zero-indexer-hub`** queues diverted transactions from every shim. Every 20 blocks it publishes them, shuffled and in parallel. Only block height starts a flush, because with a count trigger an attacker could flood the queue to isolate a target. The hub admits a migration only if it is still valid after the next flush. It refuses a migration past its lane's share of the flush budget, so one batch fits in one block.

## Status

**Deployed:** classification and diversion; the hub queue and flush; `GetTransaction` at the hub; attested Nitro enclaves with TLS inside (since 2026-08-01, first third-party operator 2026-08-10); and the Nym transport on the public mixnet (since 2026-08-14).

**Partly built: multi-hub replication.** The shim offers each migration to every configured hub, and never fails over. If it chose a hub by health, an attacker could take a hub down during one migration's retry to isolate it. The shim reports success after the first hand-off, so under mixnet backpressure only part of the list may hold the migration. Still to build: holding a migration across requests.

**Designed, no code yet:** a shim-side check of the hub's attestation (STEVE or RA-TLS); encryption to the hub's key on the clearnet hop; the keymaker quorum and consortium governance; confirmation tracking.

On 2026-08-11, a real migration from Orchard to Ironwood crossed the full stack on mainnet, and the operator's indexer never saw it. That run used the clearnet hop and a batch of one. It was a test of the mechanics and of content privacy. Batching anonymity is still untested. No one has yet observed a migration cross Nym in production.

## Maintainers

- Mark Henderson ([@aphelionz](https://github.com/aphelionz))
- Shoaib Ahmed ([@hu55a1n1](https://github.com/hu55a1n1))

Both work at [Shielded Labs](https://shieldedlabs.net).

## Thanks

Diagrams by Zooko ([zero-indexer-diagrams](https://github.com/zookoatshieldedlabs/zero-indexer-diagrams)), with the Nym hop added. Thanks to Caution and to Nym.

## Contributing

Ask questions and report bugs in [GitHub issues](https://github.com/ShieldedLabs/zero-indexer/issues). Do not report a vulnerability in an issue. Follow [`SECURITY.md`](./SECURITY.md).

Pull requests are welcome. [`CONTRIBUTING.md`](./CONTRIBUTING.md) gives the checks to run and the requirements for a pull request.

The contribution we want most is a review of the architecture: start with [Security](#security). Open questions with Caution are in [`OPEN-QUESTIONS.md`](./OPEN-QUESTIONS.md). Releases follow [`RELEASING.md`](./RELEASING.md).

## License

`MIT OR Apache-2.0` © 2026 [Shielded Labs](https://shieldedlabs.net)

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](./LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. The vendored crates in `vendor/` are under their own licenses.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
