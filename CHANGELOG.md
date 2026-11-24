# Changelog

zeronym release notes. One `## vX.Y.Z` section per release, newest first.
Stage upcoming entries under `## Unreleased`.

Releases through v29 shipped from the zero monorepo, with their notes in its
changelog under `zero-indexer`.

## Unreleased

### Changed

- The hub's first read of the Orchard nullifiers since NU6.3 is about 3.5
  times faster. It now reads 5 000 blocks per `GetBlockRangeNullifiers` call,
  up from 1 000. The hub dials a new connection for each call, so the number
  of calls set the time. Against an indexer 50 ms away, the read took 53 s
  before and 15 s after. The largest response, the 5 000 blocks right after
  NU6.3 activation, is 3.1 MB, inside the 4 MiB body limit.
- The hub schedule follows the 25-second blocks of NU7 and keeps its
  pre-NU7 wall-clock times. The flush interval is 60 blocks (about 25
  minutes), the mining margin 12, the delivery lag allowance 18 and the
  holdoff 22. The supported wallet expiry is 120 blocks, the ZIP 218
  default from NU7 (`NU7_DEFAULT_TX_EXPIRY_DELTA`). Wallets must stamp an
  expiry of 120 blocks or more. Operators set a stop grace period of at
  least 15 minutes.

## v0.1.0 - 2026-11-20

### Security

- The shim, hub and probe take `webpki-roots` 1.0.10 and
  `rustls-webpki` 0.103.16 for the eMudhra intermediate CA key compromise
  disclosed 2026-10-20. `webpki-roots` drops the emSign ECC Root CA - G3 root,
  and `rustls-webpki` rejects any chain through the eight compromised
  intermediates' public keys. Both hops build their trust store from the
  bundled roots, so a certificate forged with those keys for a hub or backend
  hostname verified until now. `webpki-root-certs` moves to 1.0.10 as well. The
  mixnet-driver build also links nym-sdk's `rustls` 0.21 stack, whose
  `webpki-roots` 0.24 and 0.25 got no fix: `vendor/` carries both with
  emSign ECC Root CA - G3 and AAA Certificate Services removed, bound through
  `[patch.crates-io]`. That stack's reqwest 0.11 also reads the system bundle,
  so the shim and hub images drop emSign ECC Root CA - G3 from the pinned
  StageX `ca-certificates.crt`.
- One indexer could steer the hub's batcher through the chain tip
  it reports. A jump past the next flush boundary flushed the queue on demand,
  a tip oscillating inside the reorg allowance froze the cadence, and a height
  near `u32::MAX` refused every expiring migration until restart. Only a new
  high now counts as the tip advancing, the reported height is range-checked,
  and on mainnet each answer must sit between a compiled anchor height and a
  clock-driven ceiling, and climb at a rate the chain can plausibly reach,
  before the hub takes the highest. One operator running every indexer keeps a bounded lever, listed in
  the hub's `OPERATORS.md`. Reported by [@ouicate](https://github.com/ouicate).
- The shim forwarded the second transaction of a TEX payment to
  the operator. Paying a ZIP 320 (TEX) address from shielded funds takes two
  transactions, and the second spends the first's single-use transparent
  output. When the first touched Orchard the shim diverted it, but the second
  has transparent parts only, so it reached the operator's indexer, naming the
  diverted transaction's txid by its prevout and tying it to the wallet's
  connection, broadcast-only mode included. The classifier now also diverts
  every transaction that spends a transparent output and has transparent parts
  only. Every such send now waits for a hub batch, including TEX payments
  funded from Ironwood. A transparent spend that also carries a Sapling or
  Ironwood bundle still forwards; `ENDPOINTS.md` lists that residual. Reported
  by [@ouicate](https://github.com/ouicate).
- The README listed source-IP protection as volume-independent.
  That holds against the hub and the peer-to-peer network. Against the
  operator and the wallet's ISP it is batching anonymity: both see each
  wallet's upload, the operator also sees the `SendTransaction` missing from
  its indexer, and on the clearnet hop the shim's fresh 64 KiB dial to the hub
  inside the wallet's request. At today's batch size of one, the next flush
  names the transaction. In a larger flush the set is the transactions that
  match in size, shape, anchor and expiry. The README and both operator guides
  now state this link and which marker Nym's cover traffic removes, and
  `guards.sh` keeps the claim from returning. Reported by
  [@ouicate](https://github.com/ouicate).
- One free burst of junk held the hub's queue for hours.
  Unparseable bytes enter the queue with no expiry, and 1024 of them (64 KiB)
  fill its entry cap. A node answers such bytes with `-22`, the deserialization
  error, and the hub retried that answer, so each junk entry went back into the
  queue for 8 more flushes (about 3.3 hours) and reached every indexer 9 times.
  The hub now reads that answer as a rejection: the `-22` lightwalletd relays,
  and zaino's `malformed transaction` rejection inside a gRPC INTERNAL (the hub
  now percent-decodes `grpc-message`). A burst now lasts one flush window. Any
  submitter can still fill the cap for that window. Reported by
  [@ouicate](https://github.com/ouicate).
- The auditor procedure left the hub hop unchecked. A clearnet
  shim authenticates its hub by the certificate name alone, so an auditor must
  tie the shim's measured hub address to an attested hub, and the four
  component guides stopped at `caution verify`. `README.md` now holds
  the full procedure in five steps, with a new parameter check (hub address,
  Nym addresses, hub indexers, `ZIS_DIAG`), and every guide links to it. The
  README now says the attestation covers an auditor's own connection while
  wallets check certificates against the public CAs only, names Caution's
  in-enclave Caddy as trusted, and notes that Nym mode encrypts each frame to
  the hub's in-enclave key. The hub deploy guide used one indexer as both a
  shim backend and the hub's lookup indexer, which hands that indexer every
  txid that misses the hub's queue. Guard M14 now compares addresses and
  certificate names across `deploy.env.example` and every assemble example in
  the docs. Reported by [@ouicate](https://github.com/ouicate).
- A clearnet shim read every `404` from the hub's lookup path as
  "no such transaction" and told the wallet gRPC `NOT_FOUND`. A hub with that
  path closed, a proxy, or any other server at the hub's address answers `404`
  for a path it lacks, so the wallet heard that every txid, mined ones
  included, was gone. Only a `404` with the hub's `transaction not found` body
  now means absence; any other `404` fails closed as `UNAVAILABLE`, as the
  mixnet path already did.
- Every hub served its clearnet lookup route, `POST /transaction`,
  to anyone, so anyone holding a candidate txid could ask whether it sat in the
  queue. A Nym shim looks up over the mixnet, so on a Nym hub the route served
  only strangers. It now closes with the clearnet submit path, behind
  `ZIH_HTTP_SUBMIT`. A clearnet pair keeps it, and the mixnet lookup still
  answers the same question; `README.md` lists that residual. Deploy
  clearnet shims with the strict lookup `404` (above) before this hub: an older
  shim reads the closed route's `404` as "no such transaction". Reported by
  [@ouicate](https://github.com/ouicate).
- Junk now costs a Halo2 proof to put in front of a migration.
  The hub verifies the Orchard and Ironwood proofs and signatures of every
  submission that touches Orchard, against the NU6.3 key it builds at boot,
  and refuses one that fails. The entry cap splits into lanes: 896 entries for
  verified transactions, 112 for transparent-only spends, 16 for what the hub
  cannot verify (unparseable bytes, an unknown circuit era, an Orchard
  transaction with transparent inputs). Free junk fills only its own lane, and a TEX
  second leg rides in the verified lane when the hub holds its parent. One
  proof buys one entry: re-signing it collapses to the held entry, and a proof
  the network rejected, or already mined, is refused for the life of the
  process. About 9 ms of CPU per migration on the enclave's four cores, three
  checks at a time. Reported by [@ouicate](https://github.com/ouicate).
- A copy of a mined Orchard transaction filled the hub's queue for
  free, because its proofs verify. About 1,740 such transactions were still
  unexpired at the tip on 2026-10-28, since a ZIP 318 migration expires 30 to 60
  days out. The hub now reads the Orchard nullifiers each indexer reports mined
  since NU6.3 (`GetBlockRangeNullifiers`, 251,421 on 2026-10-29, about a minute
  from cold), keeps them current every 30 seconds, and refuses a transaction
  whose nullifier every indexer reports. Unanimity, so one lying indexer cannot
  block a wallet's resend; an indexer that is down lets copies through to the
  burn set. Reported by [@ouicate](https://github.com/ouicate).
- The hub took any number of inbound connections. Each one holds
  a descriptor for its whole life, and one request every 29 seconds keeps it
  open, because hyper re-arms its 30-second header timeout on every keep-alive
  request. Enough of them took every descriptor, and the flush's publish dials
  then failed: each migration a wallet had already been told was sent requeued
  until the hub dropped it, and admission refused new ones as `TipStale`. The
  hub now holds at most 256 inbound connections, the shim's bound, and closes
  any over it. The mixnet listener sits outside the bound. Reported by
  [@ouicate](https://github.com/ouicate).
- One HTTP/2 connection could make the shim buffer about 800 MiB.
  The shim bounded connections (256) and each `SendTransaction` body (4 MiB for
  30 seconds), but hyper serves 200 streams on each connection, so three
  connections passed the 2048 MB enclave, and an out-of-memory kill loses every
  migration acknowledged but not yet sent. Request bodies now share one 256 MiB
  budget across every connection and stream. A body is charged as its bytes
  arrive, again for its decoded copy before decoding, and for its trailers, and
  a request over the budget gets `UNAVAILABLE`, which a wallet retries. Each
  chunk is copied into one buffer on arrival, because an h2 chunk kept its whole
  read buffer alive. Reported by
  [@ouicate](https://github.com/ouicate).
- A forwarded `SendTransaction` body returned its share of the
  shim's budget when the handler returned, while h2 could still hold all of its
  bytes. An upstream that answered and then kept its flow-control window shut
  and the stream open held up to 4 MiB per request outside the 256 MiB budget.
  The forwarded bytes now own their charge, so it is returned only when h2
  drops them: once the upstream has taken them, or the stream or connection has
  ended. Found in review of the fix above. Reported by
  [@ouicate](https://github.com/ouicate).
- A wallet that never read its responses made the shim hold the
  indexer's data: 8 MiB for each wallet connection, so 2 GiB across the 256
  allowed connections, the whole 2048 MB enclave. A wallet that stopped reading
  its socket made hyper queue 400 KiB on each of 200 streams, about 80 MB for
  each connection. The indexer leg's HTTP/2 window is now 1 MiB per connection
  and 256 KiB per stream, the shim queues at most 16 KiB per stream (about
  32 KiB with the frame hyper holds), and a connection carries at most 32
  streams. A connection now holds at most 2 MiB, so 256 connections hold
  512 MiB, and 768 MiB with the 256 MiB budget for buffered request bodies.
  The shim serves 8,192 streams at once. The cost is `GetBlockRange`
  throughput: at a 30 ms round trip to the indexer, one stream measured
  6 MiB/s (was 48 MiB/s) and four streams on one connection 22 MiB/s together
  (was 148 MiB/s). The wallet leg's request windows are unchanged. Found while
  verifying an external report on the shim's request buffering.

### Changed

- The shim serves gRPC-web (`application/grpc-web+proto` and
  `grpc-web-text`) beside plain gRPC on the same port, so browser and
  extension wallets can use it. Translation happens before routing, so a
  gRPC-web `SendTransaction` is classified and diverted like any other; the
  CORS preflight is answered by the shim; `Origin` and `Referer` are stripped
  before the indexer.
- The shim gains a broadcast-only mode (`--broadcast-only` /
  `ZIS_BROADCAST_ONLY`, off by default): only `SendTransaction` and
  `GetTransaction` are relayed, and every other gRPC method is answered
  `UNIMPLEMENTED` without reaching the indexer.
- The hub judges admission at the boundary of the flush epoch the cadence last
  recorded when that is above the observed tip, so an entry is no longer
  accepted counting on a flush that was already spent (after a tip dip, or
  after the free-running clock ran ahead of a stale tip).
- The hub treats an indexer lookup answer with no transaction body as that
  endpoint failing, so it is no longer served as "found, height 0, no bytes",
  the reply reserved for a transaction the hub itself is holding.
- The hub requires `--network` / `ZIH_NETWORK` (`mainnet`, `testnet` or
  `regtest`). Mainnet turns on the tip guard and refuses to start with a clock
  before the compiled anchor. `assemble-caution.sh` takes `--network`, and
  `deploy.sh` reads `NETWORK`.
- The hub's `OPERATORS.md` and `README.md` document a known gap: any
  submitter can fill the hub's queue for free, since 1024 junk payloads of 64
  bytes fill its entry cap, and over the mixnet each refusal while it is full is
  silent. Unparseable junk holds the queue for one flush window; forged parseable
  junk is retried and holds it for up to nine. `OPERATORS.md` also corrects two stale claims: a hub
  restart makes every shim lose its diverts silently, and the 1024-entry cap
  binds before the 64 MiB byte budget. Reported by
  [@ouicate](https://github.com/ouicate).
- The shim and hub parse NU7 transactions. They move to `zebra-chain` 15.
  Before this every transaction since NU7 failed to parse, so the shim
  diverted all of them and the hub could verify none.
- The hub's tip guard expects NU7's 25-second blocks, anchored at the
  activation block. It expected 75 seconds before, so it dropped the honest
  tip within about half an hour of NU7 and then refused every send as
  `tip_stale`.
- The hub calls its tip stale after 12 target block times, 5 minutes at 25 s.
  It was 15 minutes, which is 36 blocks now and would hold a stalled cadence
  back for more than a flush interval. An honest gap that long comes about
  once in 47 days, and the hub refuses submissions until the next block.
- A shim with a hub refuses a V1 to V4 transparent-only spend with a
  `SendResponse` error. ZIP 2003 retired V4 at NU7, so the hub could only hold
  one for a flush window and drop it, after the wallet had been told it was
  sent.
- The hub's `/healthz` follows its chain tip. It answers `503` while the hub
  would refuse a send as `tip_stale`: until the tip warm-up ends, and whenever
  the tip is stale. It answered `200 ok` through NU7 while it refused
  everything.
- A shim with a hub answers `503` on `/healthz` once the hub has handed back a
  transaction on a consensus branch the shim's build does not know. That is
  what a shim built before a network upgrade sees, and until now only a log
  line said so. Nothing a wallet sends can set it.
- A shim with a hub refuses a V5 or V6 transaction on a consensus branch its
  build does not know, with a `SendResponse` error. It used to divert it like
  any bytes it could not parse, so a shim built before a network upgrade would
  have diverted ordinary payments too, into a hub lane that holds 16 a flush
  window. Other bytes it cannot parse are handled as before. The per-send log
  line says `diverted_in_production=false` for it, and for a retired format
  that is refused.
- The hub budgets each flush by ZIP 218 shielded cost. From NU7 a block holds
  330 units, and a flush carries at most 280: 248 for the proven and
  transparent lanes, and 32 for the unverified lane. Admission answers
  `queue_full` when a transaction would take its lane over that budget. After
  each relay, the hub holds the queue until the tip clears the reorg allowance
  and the mining margin. Set the hub's stop grace period to at least 10
  minutes, because the final flush on shutdown waits for that hold.

### CI

- `zeronym-latest-blocks` runs daily. It reads 40 recent mainnet and testnet
  blocks through a shim and fails if the `zebra-chain` the shim pins cannot
  parse a transaction in them. NU7 was live on testnet for 32 days before it
  broke parsing here on mainnet, and nothing was looking.
- `zeronym-hub-indexers` runs daily. It runs `hub/tests/live_chain.rs`
  against two mainnet indexers, a lightwalletd and a ztreamer on a Zakura node.
  The hub's tests on each push use a mock indexer, so this run is where a
  change in an indexer's replies shows. `zeronym-latest-blocks` also reads
  mainnet through the ztreamer.
