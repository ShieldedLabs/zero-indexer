# The zeronym protocol, specified in Quint

A [Quint][quint] specification of zeronym: the protocol between a wallet, the shim in front of an operator's indexer, the hub that batches diverted transactions, and the chain.

> [!NOTE]
> Checked against: `main` at `89a0310`. A citation marked with another commit or PR gives line numbers there.

- [System][system]
- [Assumptions][assumptions]
- [Out of scope][out-of-scope]
- [Threat model][threat-model]
- [Guarantees][guarantees]
- [Trust matrix][trust-matrix]
- [Known gaps][known-gaps]
- [Findings][findings]
- [How it is checked][how-it-is-checked]
- [Future work][future-work]

## System

> Who the actors are, what each one does, and where each lives in the specification.

The architecture diagram and the prose description of the deployment are in [`README.md`][zeronym-readme].

Only the mixnet transport is modelled; the HTTP transport is out of scope (see [Out of scope][out-of-scope]).

There are two specifications, sharing one hub function:

- The [protocol spec][protocol-spec] has every actor, but its hub is abstract: a queue that can flush at any time. Random simulation and scripted runs check this spec.
- The [hub spec][hub-spec] models one hub concretely along with the chain, and the two things the hub asks its indexer: the tip, and a verdict on each broadcast. It owns everything the abstract hub leaves out: the schedule, expiry, requeue, crash and drain. TLC visits every reachable state of each of its configurations.

Each component is one total function from its state and one input to its next state and one output. An input that is invalid in the current state returns an error output and leaves the state alone. The state machines hold no protocol logic: a step picks an input, calls the function, and puts the output where it goes. `hub.qnt` and `shim.qnt` are the precise statement of what each does.

<details>
<summary><b>Indexer, network and observer</b></summary>

- **Chain and indexer** (`indexer.qnt`). A transaction's status only moves forward: absent, in the mempool, mined. The indexer answers lookups and gives a verdict on each broadcast.
- **Network** (`spells/soup.qnt`). A message is added to the soup and never removed. It may be delivered any number of times, in any order, or never.
- **Observer.** A client of the hub's public, unauthenticated address. It may learn a txid out of band, look it up, and resubmit any payload the chain has published.

</details>

<details>
<summary><b>Wire</b>: the three lookup replies and what the wallet sees</summary>

A hub answers a lookup with one of three wire replies, and the shim turns that into what the wallet sees (`wire.qnt`).

| The hub's situation | Reply on the wire | What the wallet sees |
|---|---|---|
| The transaction is queued here | found, height 0, no body | Pending |
| Its indexer has the transaction | found, with the height and the body | The transaction, if the body's txid is the one asked for; otherwise not found |
| Its indexer says not found | not found | Not found |
| Its indexer cannot be reached | error | Unavailable |
| Its indexer says "found" with no body, at any height (a fault) | error | Unavailable |

</details>

## Assumptions

> What must be true of the world for the claims to apply.

- **Roles.** An honest component takes exactly the transition its function gives. A Byzantine one takes any member of a finite set that contains the honest transition (`byzantineContainsHonestTest`), and no message or state field records which it took. The set holds protocol moves only, so a component that leaks what it sees (by logging it, or through extra bytes, sizes or timing) falls outside it; see [Out of scope][out-of-scope]. The shim and the hub both run attested. An honest shim or hub runs a binary built from the commit under *Checked against*, which reproduces to the hash in its `deploy/EXPECTED_SHA256` and passes the auditor procedure in [`AUDITING.md`](../../AUDITING.md) (`caution verify` is its step 2). The shim is modelled as honest in every configuration: it sees every migration in plaintext and controls everything the wallet observes, so no wallet-facing guarantee could survive its compromise. The hub is modelled as honest or Byzantine to measure how much each guarantee depends on the hub's enclave. The indexer runs outside any enclave, so a Byzantine indexer is the realistic adversary. One component is Byzantine at a time.
- **Byzantine hub** (protocol specification only). It admits or refuses a submission whatever the admission rules say, and may send any reply to a lookup. Its ack is modelled as truthful: a real one could ack anything, but nothing reads an ack. Every other move is the honest one: it cannot evict or withhold a queued entry, flush off schedule, or send a frame nobody asked for.
- **Byzantine indexer.** Any verdict, with the transaction relayed or not. Any answer from its nullifier view. Any lookup answer built from a payload it was offered or one the chain published. In the hub specification it also reports any tip in the model's range. On mainnet ShieldedLabs/zero#92 drops a tip outside a band or above the plausible climb rate (`tip_guard.rs`), so this over-approximates the guard.
- **Network.** May lose, duplicate, delay and reorder frames. Cannot forge or read them.
- **Observer.** Looks up txids it knows and submits payloads it has learned or the chain has published. It cannot read or forge frames, so it does not know a nonce and cannot answer the shim.
- **Nonces** are unique. A counter stands for an unguessable value.
- **Chain.** No reorg of an included transaction and no mempool eviction. The operator's indexer publishes nothing. Each block holds at most 330 units of ZIP 218 shielded cost (`blockActions`). Other traffic takes up to `maxCompeting` of them. The miner is honest and fills each block: it adds mempool transactions until no other one fits in the remaining shielded cost, and the choice among those that fit is free.
- **Wallets.** A supported ("conforming") wallet sets an expiry at least `MIN_WALLET_EXPIRY` after the height it builds at, and its frame reaches the hub within `MAX_DELIVERY_LAG` blocks. A wallet asks only about transactions it has sent. The model also has wallets below the floor and submissions later than the delivery lag.
- **Hub schedule** (hub specification only). How promptly the hub learns the tip depends on the configuration: at every block, up to the reorg allowance behind, or not at all for a while. Fewer blocks arrive while a flush is in flight than the mining margin reserves. The code enforces neither (findings 1, 2 and 4). The candidate fix of ShieldedLabs/zero#121 also needs three relations (`budgetFitsBlock`). The per-flush budget is at most 330 minus the competing reserve, which the hub's configuration sets. The mining margin is at least one block more than the longest flight. The flush interval is at least the reorg allowance plus the mining margin: 20 >= 14 on the shipped constants.
- **Time.** There is no clock. A timeout may happen at any moment; the staleness window is counted in blocks.

## Out of scope

> What the specification deliberately does not cover, and why.

### Not modelled

| Item | Reason |
|---|---|
| Several hubs | Production replicates: every shim sends every submission to every hub (`shim/src/nym.rs:602-647`). Not checked as a result: two hubs disagreeing about one transaction; duplicate publication and a second enclave holding the plaintext, both accepted in production; told ok after a partial send; a lookup moving to the next address on a timeout (finding 9); one Byzantine replica breaking G2 and G4 |
| Attestation, PCRs, TLS, STEVE | No in-protocol messages exist. Represented by the roles |
| Mixnet internals; the shim's client rotation; both `nym_driver.rs` | Their protocol-visible effect is loss and delay |
| The hub's lookup concurrency bound, reply deadline and dropped acks | Refinements of "the network lost the message" |
| Multiple indexer endpoints | One abstract indexer stands for all of a hub's endpoints. The trust matrix says, for each Byzantine-indexer cell, whether one lying endpoint suffices |
| Wire codecs, byte layout, malformed frames | Pinned by the Rust tests and golden vectors in both crates. The specification works at the level of what a reply means, and does not bind the codec |
| Anonymity-set size, shuffle, simultaneity | Not properties of a single run |
| Covert channels: extra bytes on the real wire, sizes and timing | Messages are strictly typed. A message can carry only its type's fields, so one with extra data is unrepresentable in the model |
| A transaction's inputs and outputs beyond the txids it spends from: output indices, values, scripts, addresses | A payload carries the txids its transparent inputs spend (`spends`) and what the shim's parser reads of its pools (`class`). Sprout, Sapling and Ironwood are one class, `OtherShielded`, because the classifier treats them alike |
| Transaction dependencies at the node | The indexer judges each transaction on its own. ShieldedLabs/zero#94 queues both legs of a TEX payment at the hub, and one flush may offer the second leg before the first; what a node answers then is outside the model |
| What the hub's verdict and ack say, and the HTTP transport that waits for the verdict (`HubTransport::Http`, `--hub`) | The deployment is the mixnet (`HTTP_SUBMIT=0`): the shim tells the wallet ok without waiting, and nothing reads an ack. So in every configuration the shim's ok says only that the frame left the shim. That an accepted ack is only for a queued payload is unchecked, and a Byzantine hub's ack is modelled as truthful |
| The hub's capacity and size refusals, the burn set's bound, and denial of service generally | Not claimed properties. The shim's own too-large refusal is modelled. A free burst of junk fills the entry cap for a flush window: finding 13. A replay of published bytes can fill the per-flush budget, as it can the entry cap. Two external reports exhausted resources outside the model: HTTP/2 streams made one connection buffer about 800 MiB in the shim (ShieldedLabs/zero#115), and idle connections took the hub's descriptors so its publishes failed (ShieldedLabs/zero#116). Each ends in a loss already recorded: K1 for a shim that dies after telling a wallet ok, K5 for a hub that cannot publish |
| An observer submitting payloads of its own making | Possible, since the hub's address is public and unauthenticated. The model's observer submits only what it has learned or the chain has published. Finding 13 is what junk of its own making buys |
| A different transaction with the same txid (a twin) | No configuration contains one. The pure wire test shows the shim would serve it (`servedOnlyOnMatchingTxidTest`): the shim compares the txid and nothing else |
| More than one Byzantine component at once | The trust matrix is single-fault |
| A batch mined across several blocks | It splits the batch in time. Anonymity sits outside trace properties (T17) |
| Mined status in what the indexer reports | Blocks record what they include in `mined`, which G6c reads. The indexer's view (`chain`) keeps every relayed transaction in the mempool, so a lookup never answers mined and a resend never meets `AlreadyMined` |
| Fee-order and byte-limit block selection | A transparent transaction has shielded cost 0. A miner that orders by fee can fill the 2 MB block with them and take no hub transaction |
| Liveness: that anything eventually happens, such as a submitted migration being published | The network may lose everything, and nobody waits for an ack |

## Threat model

> What the specification is afraid of, who could cause it, and whether a property answers it.

The deployment targets the server-side and network-metadata adversaries of Taylor Hornby's [wallet app threat model][wallet-threat-model]. What is protected, and what stays exposed, are in [`THREAT-MODEL.md`](../../THREAT-MODEL.md). Of that, only the properties of protocol runs are modelled here. Under [Guarantees][guarantees], each threat answered by a guarantee has its id in the "Answers" column. Under [Known gaps][known-gaps], each threat with a recorded gap has its id in the "Threat" column.

| # | Threat | Adversary | Status |
|---|---|---|---|
| T1 | The operator sees a migration's contents | Operator behind the shim | Answered |
| T2 | Someone obtains a queued migration's bytes and publishes it early, breaking the batch | Unauthenticated observer; Byzantine hub or indexer | Answered, if the hub and the indexer are honest |
| T3 | The wallet is served a different transaction than the one it asked for | Byzantine hub or indexer | Answered for the txid; not for the bytes or the height |
| T4 | The wallet is told something false about its transaction's status | Byzantine hub or indexer; network reordering | Answered for each answer, if the hub and the indexer are honest; not across answers (T8) |
| T5 | An admitted migration expires while the hub holds it | Chain timing; a flaky tip; Byzantine hub or indexer | Answered in part for supported wallets: under a timely or regressing tip, not under a stale one. Not checked for late arrivals and unsupported wallets |
| T6 | The hub silently drops or admits entries outside its rules | Hub implementation error | Answered |
| T7 | The wallet is told "sent" but the hub never admits it | Network; the hub's own refusals; an observer's burst of junk (finding 13) | Not prevented |
| T8 | The wallet sees its transaction's status go backwards | Network reordering; resubmission; the flush window (after a flush empties the queue, before the indexer has the batch) | Not prevented |
| T9 | An acknowledged migration is lost to a crash, a failed final flush, or a requeue drop | None needed | Not prevented |
| T10 | An observer who knows a txid learns it is queued | Unauthenticated observer | Accepted. A Nym hub answers on the mixnet only; a clearnet hub also answers on `POST /transaction` (ShieldedLabs/zero#99) |
| T11 | A lying indexer makes the hub flush early, shrinking the batch, or stop admitting | Byzantine indexer; one endpoint suffices | Bounded on mainnet by ShieldedLabs/zero#92 (finding 11); possible inside the bound; recorded |
| T13 | The hub acknowledges a migration it never queued | Byzantine hub | Not modelled: nothing reads an ack |
| T14 | Linking a wallet to its migration by source IP | Observer; operator | Not modelled, except the route through a forwarded transaction (T19). `THREAT-MODEL.md` has the claim against the hub and the peer-to-peer network at any volume. The operator's and the wallet's ISP's link at batch size one is listed there as not protected (T17) |
| T15 | Linking by submission size and arrival time | Operator | Not modelled. Listed as not protected in `THREAT-MODEL.md` |
| T16 | The operator recovering txid and value through transparent-pool queries | Operator | Not modelled: the queries are outside the divert protocol. Listed as not protected in `THREAT-MODEL.md`. The same txid reaching the operator in a forwarded `SendTransaction` is T19 |
| T17 | Batch-size and timing anonymity; partitioning the anonymity set across hubs | Observer; operator | Out of scope: timing and anonymity are not trace properties here, and there is one hub |
| T18 | A compromised shim or enclave host | Host; a malicious build | Assumed away. A compromised host is delegated to AWS and Caution in `THREAT-MODEL.md`; a malicious shim build is excluded by the auditor procedure in `AUDITING.md` and is not discussed further |
| T19 | The operator learns a diverted transaction's txid on the wallet's connection, from a forwarded transaction that spends its output | Operator | Answered for a transparent-only spend, the second leg of a TEX payment (G9, by ShieldedLabs/zero#94). Not prevented for a spend with a shielded bundle (K10) |
| T20 | A substitute hub reads migrations on the clearnet hop: the shim checks only the certificate name for the hub address in its config | A network position on the shim-to-hub path plus a certificate for the hub's name: Caution or AWS on a managed app, a parent-host operator in BYOC, with whoever holds the hub's DNS zone | Assumed away by the honest-hub role. Listed as not protected in `THREAT-MODEL.md`. In auditor step 4 (`AUDITING.md`), the auditor ties the address to an attested hub. In Nym mode, the Nym client encrypts each frame to the hub's in-enclave key (ShieldedLabs/zero#98) |
| T21 | A second valid certificate for the wallet-facing name serves wallets while auditors reach the enclave | The operator, who holds the wallet-facing DNS zone, from a network position on the wallet's path | Assumed away by the honest-shim role. In `THREAT-MODEL.md`, the attestation is scoped to the auditor's own connection (ShieldedLabs/zero#98) |
| T22 | The operator sees the txids wallets look up, through the hub's lookup fall-through to an indexer it runs | Operator, when it also runs the hub's indexers | Not modelled: the indexer here is one party, distinct from the operator. The condition is stated in `THREAT-MODEL.md`. Guard M14 checks it, and so does the auditor in step 4 (ShieldedLabs/zero#98) |
| T23 | The indexer gives a final verdict on a transaction it did not relay. The hub drops its only copy; a `Rejected` or `AlreadyMined` verdict also burns the txid, so the wallet's resend is refused until the hub restarts | Byzantine indexer. For the drop, one endpoint, when no honest endpoint relays (the best verdict wins, `chain.rs`, `best_of`); for the burn, every endpoint that answers | Not prevented, and not checked: K5 counts a final verdict as settled |

## Guarantees

> Promises about whole runs of the system: this bad thing never happens.

| Id | Name | What it says | Answers | Checked by |
|---|---|---|---|---|
| G1 | `operatorBlind` | Every transaction the shim forwards to the operator's indexer is a Sprout, Sapling or Ironwood transaction, or a coinbase, read from its pools and not from the shim's classifier | T1 | Simulation |
| G2 | `queuedBytesConfidential` | Everything the observer has learned is on the chain, or was a Sprout, Sapling or Ironwood transaction or a coinbase given to the operator's indexer. Its knowledge is derived from the replies sent to it and the operator's view | T2 | Simulation |
| G3 | `txidAuthenticity` | A transaction served to the wallet has the txid asked for. It need not be the bytes the wallet sent, and its height is whatever the hub said | T3 | Simulation; `servedOnlyOnMatchingTxidTest` exhaustively |
| G4 | `lookupValidityPerHub` | Every lookup answer other than "unavailable" was true at the hub that gave it at some point between request and answer. Not-found during the flush window counts as true, as the implementation intends (`Hub::lookup`, `hub/src/server.rs`). It does not say that successive answers agree | T4 | Simulation |
| G5 | `pendingOnlyFromQueue` | Every "pending" the wallet is told was true at the hub that gave it: the transaction was queued there. G4 restricted to pending | T4 | Simulation |
| G10 | `oneAnswerPerLookup` | The wallet gets at most one answer per lookup the shim sent. G4 reads its window from the shim's waiters, so a second answer is invisible to it |  | Simulation |
| G9 | `forwardedSpendsNoDiverted` | No transaction the shim forwards to the operator's indexer spends an output of one the shim diverted, in either order and before or after publication. Holds while every such spend has transparent parts only; a spend with a shielded bundle is K10 | T19 | Simulation; `texSecondLegIsDivertedTest` |
| G6b | `conformingFirstOfferBeforeExpiry` | A supported wallet's transaction is offered with the mining margin to spare, the first time a hub offers it. About the margin left when the flush begins, not about acceptance; nothing about a later offer of a requeued entry | T5 | TLC, exhaustive |
| G6c | `conformingMinedBeforeExpiry` | A supported wallet's transaction that a node took is mined before it expires. Stronger than G6b: mined, where G6b says offered with the margin | T5 | TLC, exhaustive |
| G8 | `admissionMatchesCadence` | Every entry admitted, and not requeued, survives the first flush the cadence has not spent, with the mining margin to spare against the cadence boundary. G6b is the margin at the chain height | T5 | TLC, exhaustive |
| G7 | `wellFormedTest` | Structural sanity of the hub: a queued entry is within its attempts, a down hub holds nothing and knows nothing, and a stopped hub holds nothing | T6 | Exhaustive test over every reachable hub state |
| A2 | `neverEvictTest` | An entry leaves the hub's queue only into a flush, because a node judged an entry with its txid and the hub burned that txid, or because the hub went down or exited after its final flush | T6 | Exhaustive test over every reachable hub state and input |
| A3 | `drainIsFinalTest` | A draining honest hub's queue gains only what a flush hands back | T6 | Exhaustive test over every reachable hub state and input |

An unchecked corollary of G6b: if fewer blocks arrive while a flush is in flight than the mining margin reserves, a node that judges a supported wallet's first offer finds it unexpired. Nothing in the code bounds a flight in blocks (finding 4).

A2 and A3 constrain a single hub step, not a state; the rest are state invariants. What each entry under "Checked by" is worth is under [How it is checked][how-it-is-checked].

Each pure function also has exhaustive tests over small inputs, in `tests/*Test.qnt`.

## Trust matrix

> For each guarantee, whose honesty it depends on.

Single-fault. "holds" is a checked row on the named [configuration][configurations]. "required" is a scripted run in which the component is Byzantine and the guarantee fails; the run asserts the guarantee in the state just before the Byzantine step, so the lie is what breaks it.

| | All honest | Byzantine hub | Byzantine indexer |
|---|---|---|---|
| G1 | holds (`baseline`) | holds (`byzHub`) | holds (`byzIndexer`) |
| G2 | holds (`baseline`) | **required**: `hubServesQueuedBodyTest` | **required**: `indexerServesUnpublishedBodyTest`. One endpoint suffices |
| G3 | holds (`baseline`) | holds (`byzHub`) | holds (`byzIndexer`) |
| G4 | holds (`baseline`) | **required**: `hubDeniesQueuedTest`, `hubServesFalseHeightTest` | **required**: `indexerServesFalseHeightTest`. One endpoint suffices |
| G5 | holds (`baseline`) | **required**: `hubForgesPendingTest` | holds (`byzIndexer`); `indexerEmptyAnswerIsUnavailableTest` |
| G9 | holds (`baseline`) | holds (`byzHub`) | holds (`byzIndexer`) |
| G10 | holds (`baseline`) | holds (`byzHub`) | holds (`byzIndexer`) |
| G6b | holds (`timely`, `flakyTip`) | argued, not checked: a hub that admits while it has no tip breaks it | **required**: `indexerWithholdsTipFromConformingTest`. Needs every endpoint |
| G6c | holds (`timely`, `flakyTipBudget`) | argued, unchecked | argued, unchecked |
| G8 | holds (`timely`, `flakyTip`, `staleLagWithSlack`) | argued, not checked | argued, not checked: admission never judges below the recorded boundary, whatever the tip. Against the cadence boundary only |
| A3 | holds | argued, not checked: draining is an admission rule | holds |

- A hub folds several indexer endpoints into one answer: the tip is the maximum of the answers the tip guard keeps (`batcher.rs:251-288`, `chain.rs:215-247`), a lookup takes the first "found", a broadcast takes the best verdict. So one misbehaving endpoint can raise the tip (on mainnet, only inside the guard's band and climb rate), inject a lookup answer or raise the verdict to `Accepted` or `AlreadyKnown`, while lowering or freezing the tip takes every endpoint. A burn happens when every endpoint that answered says mined or rejected. The model has one abstract indexer and does not enforce that difference; the indexer cells say which each needs.
- There is no shim column: every wallet-facing guarantee assumes an honest, attested shim.
- G3 is the only wallet-facing guarantee that survives a Byzantine hub or indexer, and it authenticates the txid only.
- G1 and G9 depend on the shim alone.
- The hub specification has no Byzantine hub, so the hub column of G6b and A3 is an argument. A Byzantine hub breaks G6b by admitting while it has no tip, when an honest hub refuses everything. Admitting past the expiry rule cannot break it, because that rule never refuses a supported wallet's timely transaction (`conformingTimelyPayloadIsAdmissibleTest`). The protocol specification's Byzantine hub backs the other rows.

## Known gaps

> Things you might expect to hold that don't, each with a concrete example run. Every component is honest in all of them.

"By design" means the implementation says so itself. "Found here" means this specification showed it and nobody has yet decided whether it is acceptable. "Where" names a [configuration][configurations].

| Id | Threat | What is lost | Status | Where | Checked by | Scripted run |
|---|---|---|---|---|---|---|
| K1 | T7 | Told ok does not mean the hub ever admits it: it may refuse the frame, or never receive it | By design: the hub's verdict "is deliberately not waited for" (`shim/src/hub.rs:301-307`). An observer can make every refusal in one flush window `Full` at no cost (finding 13) | `baseline` | scripted runs | `toldOkThenRefusedTest`, `toldOkAndNeverDeliveredTest` |
| K2 | T8 | `statusNeverRegresses`: what a wallet sees of one transaction never goes backwards | By design for the flush window, which the code accepts in a comment on `Hub::lookup` (`hub/src/server.rs`); resubmission of a published transaction is finding 10, not triaged | `baseline` | simulation, violated | `repliesReorderedTest`, `walletResendsPublishedTest`, `observerResubmitsPublishedTest`, `flushWindowTest`, `rejectedAtFlushTest` |
| K3' | T5 | G6b when the expiry floor leaves no slack for the reorg allowance | Found here (finding 3). The shipped slack is now 30, three times the reorg allowance; `flakyTipNoSlack` keeps a floor with no slack | `flakyTipNoSlack` | TLC, violated | `conformingMissesMarginWithoutSlackTest` |
| K4 | T5 | G6b on the relation between the constants before NU7 (20, 4, 6, 40), across a tip silence shorter than the staleness window | Found here (finding 1); closed on the shipped constants: 90 + 11 = 101 <= 120. `staleLag` keeps the floor before NU7 | `staleLag` | TLC, violated | `silenceAcrossBoundaryMissesMarginTest`; contrast `sameSilenceWithSlackKeepsMarginTest` |
| K5 | T9 | `ackedIsHeldOrSettled`: an acknowledged payload is still held by the hub, or is on the chain, or a node judged it | By design: the queue is in memory only, and the code logs what it loses (`hub/src/batcher.rs:530-540`, `:678-689`). How much one unjudged flush drops is not triaged (finding 5) | `timely` | TLC, violated three ways | `ackedThenCrashedTest`, `ackedThenLostAtDrainTest`, `requeueDropsAckedAsExpiredTest` |
| K6 | T5 | `conformingEveryOfferBeforeExpiry`: G6b for every offer, not only the first | Found here (finding 7); accepted: the late offers normally reach no node, and a drop on the free-running clock can lose an entry the chain would still mine | `staleLag` | TLC, violated | `requeuedPastExpiryTest` |
| K9 | T5 | G6b even when the expiry floor is wide enough to cover a tip silence: a stale hub's free-running clock flushes an empty queue early and spends the next epoch, so a transaction admitted after the tip returns waits a full interval | Found here (finding 2). ShieldedLabs/zero#85 fixed admission's half (G8); the spent epoch remains, and this counterexample is unchanged | `staleLagWithSlack` | TLC, violated | `earlyFlushSpendsTheNextEpochTest` |
| K10 | T19 | G9 for a spend that carries a shielded bundle. A wallet deshields Orchard to its own address, then auto-shields that output into Ironwood: the second transaction forwards, and its prevout names the diverted first | By design in ShieldedLabs/zero#94: catching it needs a record of what was diverted, and the shim keeps none (`shim/ENDPOINTS.md:208-214`) | `shieldedSpend` | simulation, violated | `shieldedSpendOfDivertedIsForwardedTest` |
| K11 | T5 | G6b when the expiry floor covers the reorg allowance and the silence each alone. A report one below the best un-dues a flush, then a silence shorter than the staleness window delays it further: the hub flushes the allowance plus the window minus one late, while running. With the floor one block higher the shortest counterexample is K9's | Found here; not triaged. On the shipped constants the slack of 30 covers both lags (10 + 11 = 21) | `flakyStaleLagWithSlack` | TLC, violated | |
| K12 | T5 | G6c: a batch that costs more than the blocks left before its tightest expiry can mine loses a supported wallet's transaction | Found here; not triaged | `flakyTip` | TLC, violated | `batchSpillsPastExpiryTest` |

K1 is not an invariant because it would be false on the ordinary success path too: the wallet is told ok before the hub has the frame.

Behaviours that are accepted or only recorded:

| Behaviour | Threat | Shown by |
|---|---|---|
| **Accepted disclosure.** An observer that knows a txid learns that it is queued. The hub withholds the bytes, not the fact. The implementation leaves this open deliberately (`Hub::lookup`, `hub/src/server.rs`): "the 200-versus-NotFound distinction still discloses that a given txid is queued here. Closing that too means answering NotFound, which costs a wallet the ability to tell "pending" from "never seen"" | T10 | `observerLearnsItIsQueuedTest`; witness `wQueuedDisclosed` on `baseline` |
| **Premature flush.** A Byzantine indexer reports a tip ahead of the chain and the hub flushes before the true boundary: a batching harm, not an expiry one. A report far ahead of the chain is finding 11, which ShieldedLabs/zero#92 bounds on mainnet | T11 | `tipAheadOfChainFlushesEarlyTest` |
| **Early flush by the free-running clock.** A stale hub's clock is ahead of the chain and it flushes before the true boundary, with every component honest | T11, with no liar | `freeRunningClockFlushesEarlyTest` |
| **Unparseable and queued.** A payload the hub cannot parse has no txid, so a lookup misses it while it is queued | none | `unparseableIsQueuedAndMissedTest` |

## Findings

> Where the model disagrees with what the code or its comments assume.

Findings 6 and 8 are fixed by ShieldedLabs/zero#85, and finding 2 in part. Finding 11 is fixed by ShieldedLabs/zero#92, and finding 12 by ShieldedLabs/zero#94. Finding 10 is fixed in part by ShieldedLabs/zero#104 and ShieldedLabs/zero#106, and finding 13 by ShieldedLabs/zero#100 and ShieldedLabs/zero#104. Nothing else here has been fixed. "Code read" means the cited lines were read and match the model; nothing was run against the Rust. The severity beside a code bug is a judgement from that reading, not from a reproduction: HIGH, one in-scope adversary degrades batching or availability for every user of the hub; MEDIUM, one wallet loses a transaction or is misled about one; LOW, a transient wrong status or a wrong operator signal. Only code bugs carry a severity, so a row without one is not thereby minor: finding 5 is by design and may matter more than any MEDIUM bug.

| # | Finding | Kind | Shown by | Against the Rust |
|---|---|---|---|---|
| 1 | **A short tip silence costs a supported wallet its mining margin.** The cadence follows the last tip seen, so a silence across a flush boundary delays the flush until the hub goes stale. Before NU7 the staleness window was 12 blocks and the slack 10, so the first offer was at `created + 37` against an expiry of `created + 40`: three blocks of margin where four were reserved. On the shipped constants the slack is 30: the first offer is at most at `created + 89` (18 + 60 + 11) against `created + 120`, 31 blocks where 12 are reserved. It needs the hub not to hear the tip while a broadcast would still succeed, and the tip not to return before the hub goes stale; the tip and the broadcast use the same endpoints, so an unreachable indexer delays both | Latent before NU7; closed on the shipped constants | K4, `silenceAcrossBoundaryMissesMarginTest`; TLC on `staleLag` | Code read: `batcher.rs:62-93`, `:385-389`, `:426-441`. The one-block shortfall reads 15 minutes as exactly 12 blocks |
| 2 | **An early free-running flush spends the next epoch.** A stale hub's clock runs ahead, flushes an empty queue and records that epoch. When the tip returns, admission counts on a flush that has already happened, and the transaction waits a full interval. A wider expiry floor does not fix it. The supported-wallet witness also needs a second, shorter silence | Code bug (MEDIUM), partly fixed by ShieldedLabs/zero#85; one root cause with finding 6: the loop records the epoch from the cadence height, and admission estimates the next flush from the observed height | K9, `earlyFlushSpendsTheNextEpochTest`; TLC on `staleLagWithSlack` | Code read: `batcher.rs:428-441`, `:503-522`. The comment there calls a clock that runs ahead "the safe direction" |
| 3 | **The reorg slack holds by coincidence of constants.** The expiry floor minus the three-term budget (flush interval, mining margin, delivery lag) was 10 blocks before NU7, exactly the reorg allowance. It is now 30 (120 - 90), three times the allowance, and startup validation checks only the three-term sum. Without the slack a supported wallet's transaction misses its margin | Latent: no failure on the shipped constants | K3', `conformingMissesMarginWithoutSlackTest`; TLC on `flakyTipNoSlack` | Code read: `batcher.rs:62-81`, `:123-135` |
| 4 | **A flush's flight is bounded in time, not in blocks.** The budget leaves exactly the mining margin at the offer, so blocks that arrive while the batch is in flight come out of it. Each call is bounded at 10 s with 64 in flight, about 160 s at the 1,024-entry cap, which is about 6.4 nominal blocks at 25 s against a margin of twelve. Block arrival is not bounded by the clock, and requeue does not enforce the entry cap | Bounded in time, not in blocks; reachable only with a slow or selectively hanging indexer | Not a checked property: the corollary of G6b under [Guarantees][guarantees] assumes it away | Code read: `chain.rs:55`, `:65`. Estimate, not measured: with exponential block times at a 25 s mean, a flight of that length sees twelve or more blocks about one time in 33 (Poisson mean 6.4, P(N >= 12) = 0.031) |
| 5 | **An acknowledged payload can be lost three ways with every component honest:** a crash; a draining hub's final flush that finds the indexer unreachable; a requeue that gives the entry up as expired. On the shipped constants a requeue keeps an entry only if it was created at most 48 blocks before the flush (`120 - 60 - 12`). For wallets at the 120-block floor, one unjudged flush therefore drops between 20% and 50% of a batch, depending on delivery lag, each entry with 42 to 71 blocks of life left | By design; the magnitude is not triaged | K5 and its three runs; TLC on `timely` | Code read: the loss is logged at error level (`batcher.rs:530-540`, `:678-689`) and unit-tested (`queue.rs`, `a_requeue_gives_up_on_an_entry_that_can_no_longer_be_mined`); the rule is `queue.rs:854-867`. The percentages are arithmetic on the constants, assuming arrivals uniform over the interval |
| 6 | **An entry admitted while the tip is reported below a boundary already flushed waits a full interval.** Before ShieldedLabs/zero#85 admission counted on the flush at that boundary, which will not happen again. Any late arrival met it, and one that expired before that next flush was first offered after its expiry; the supported-wallet case was a resend after a crash, which a restarted hub offered with less than the mining margin. Since ShieldedLabs/zero#85 admission counts on the next unspent flush, and that resend is refused | Code bug (MEDIUM), fixed by ShieldedLabs/zero#85; one root cause with finding 2 | G8 on `flakyTip`; `crashThenLateDuplicateTest` | Code read before ShieldedLabs/zero#85, at `ee910d6`: `batcher.rs:177-186`, `:316-327`; `queue.rs:294`, `:507-520` |
| 7 | **On a stale hub a requeued entry is offered again past its expiry, and only the attempt bound stops it.** Each requeue judges the entry against the same stale observed tip, so the expiry rule never gives it up. A supported wallet's transaction is offered a third time after its expiry, and is finally dropped as exhausted. The late offers normally reach no node: a stale hub whose flushes come back unjudged almost always has an unreachable indexer | Accepted (2026-11-18): low harm, and each fix costs more. A drop judged on the observed tip adds nothing to requeue's rule, since that tip has stopped. A drop judged on the free-running clock can lose an acknowledged entry while the chain is behind the estimate (`freeRunningClockFlushesEarlyTest`) | K6, `requeuedPastExpiryTest`; TLC on `staleLag`; `expiringEntryDroppedAsExhaustedTest` | Code read: `batcher.rs:764-776`; the comment on `Requeued::dropped_exhausted` in `queue.rs` now says the expiry may have passed on a stale hub. The shipped bound is 8 requeues |
| 8 | **One indexer endpoint can make a wallet see "pending" for a transaction nobody holds.** "Found, height 0, no body" from an indexer is byte-identical to the hub's own queue-hit reply, and the hub forwards it unchanged | Code bug (MEDIUM), fixed by ShieldedLabs/zero#85: an ambiguous wire reply, and no check on an indexer's answer | `sentinelCollisionTest`, `indexerEmptyAnswerIsUnavailableTest` | Code read before ShieldedLabs/zero#85, at `ee910d6`: `server.rs:445-451`, `chain.rs:284-290`, `:305-319` |
| 9 | **Lookups choose a hub by apparent liveness.** A lookup starts at a rotating cursor and moves to the next address only on a timeout, the pattern the submit path forbids. Whoever can make one hub time out decides which hub answers | Undecided: deliberate and tested, and in conflict with a rule written for submits. The `PRODUCTION.md` that rule cites is not in the repository | Not modelled: needs more than one hub | Code read: `shim/src/nym.rs:746-797`, against the rule at `:628-633`; `shim/tests/nym.rs` asserts the behaviour today |
| 10 | **Anyone can make the hub answer "pending" for a published transaction, and pad its batch-size signal.** The queue deduplicates against resident entries only, so published bytes are admitted again if resubmitted, and a lookup answers from the queue first. The wallet reads "pending" for a transaction it has already been served, until the next flush. The flush also counts an already-known verdict toward the achieved batch size, which the code calls "the honest measure of the privacy", so free, public bytes can raise that figure and suppress the warning for a batch of one | Code bug (LOW), fixed in part by ShieldedLabs/zero#104 and ShieldedLabs/zero#106: a copy of an Orchard transaction a node rejected or mined is refused `SpentProof`. The comment on `Hub::lookup` calls a resubmit harmless. The batch-size figure is logged and returned but drives no decision | K2, `observerResubmitsPublishedTest`, `walletResendsPublishedTest`; the refusal in `admissionDecisionTableTest` | Code read: `queue.rs:367-371`, `:666-667`; `Hub::lookup` in `server.rs`; `batcher.rs:545`, `:588`, `:691`. Not checked: that a node answers already-known for a transaction that is already mined |
| 11 | **One high tip report pinned the hub's tip.** Before ShieldedLabs/zero#92, `observe` adopted any higher height and ignored a later drop larger than the reorg allowance. So one endpoint reporting a height P made every real height look like an oversized regression until the chain reached P minus the allowance, which for a large P is until restart. The epoch jumped, so the hub flushed once off cadence; after that admission refused every expiring transaction, because the observed height was P. The same rule could freeze the cadence: every step up refreshed liveness, so a tip that oscillated inside the allowance never went stale. ShieldedLabs/zero#92 judges each answer before the maximum: on mainnet it drops a height outside a band around a compiled anchor, which also refuses a height near `u32::MAX`, and a new high faster than the chain can climb. Only a new high refreshes liveness, reorg depth is measured from the best height seen, and the free-running clock starts from that height | Code bug (HIGH), fixed by ShieldedLabs/zero#92. On mainnet the guard bounds the lie: inside the band and rate one endpoint can still shift flush timing, hold a lead of about 16 blocks, or choose the boot height. Testnet and regtest are unguarded | `observeTip` follows ShieldedLabs/zero#92: `tipRuleTest`, `staleCadenceTest`, `tipNearBestTest`. The Byzantine indexer still reports any tip in a bounded range, which over-approximates the guard. The freeze was invisible to the model before ShieldedLabs/zero#92: staleness here is counted from the chain height alone, and the one configuration that goes stale reports the true height, which only climbs. `tipAheadOfChainFlushesEarlyTest` shows the one early flush | Code read: `batcher.rs:19-46`, `:169-208`, `:246-361`, `:426-441`; `tip_guard.rs:40-64`; `chain.rs:215-247`. Before ShieldedLabs/zero#92, at `2bae0a6`: `batcher.rs:19-22`, `:177-209`, `:355-374`; `chain.rs:176-201` |
| 12 | **The second leg of a TEX payment named a diverted transaction.** A ZIP 320 (TEX) payment from shielded funds is two transactions, and the second spends the first's single-use transparent output. When the first touched Orchard, the shim diverted it. The second has transparent parts only, so the shim forwarded it, and its one prevout handed the operator the txid the hub held, on the wallet's connection. An external report found it; this specification missed it. A payload carried a class label and nothing about its inputs, so G1 held by construction and every property read one send at a time. A payload now carries the txids it spends and its pool shape, the shim applies ShieldedLabs/zero#94's rule to them, and G9 states the link | Code bug (privacy), fixed by ShieldedLabs/zero#94. The severity scale grades batching and availability harms, so this row is ungraded | G9, `texSecondLegIsDivertedTest`; K10 for what ShieldedLabs/zero#94 leaves. A one-off mutation run with the transparent-only arm removed: the `baseline` simulation violates G9, and `texSecondLegIsDivertedTest` fails at its second send | Code read: `classify.rs:274-295`, `:333-338` |
| 13 | **A free burst of junk shuts admission for a flush window, and before ShieldedLabs/zero#100 for nine.** Unparseable bytes are admitted with no expiry, and 1,024 entries fill the cap, so 64 KiB of junk refuses every later submission `Full` until the next flush. Forged parseable bytes fill it as well, because the node is the only judge of validity. Over the mixnet each refusal is K1: the wallet was already told ok, and the SDK keeps a ZIP 318 migration's notes locked until its expiry, 30 to 60 days out. Before ShieldedLabs/zero#100 a node's parse-failure answer (`-22`) came back retryable through lightwalletd and zaino both, so the flush requeued every junk entry up to the attempt bound: one burst held the cap for 8 more flushes and reached every indexer 9 times. An external report found it; this specification could not, since it has no capacity and its observer submits only what it has learned | Code bug (HIGH), fixed in part by ShieldedLabs/zero#100 and ShieldedLabs/zero#104: the requeue loop is gone, and an Orchard entry costs a proof in a lane of its own. The free lanes remain, documented in the hub's `OPERATORS.md` (ShieldedLabs/zero#102) | Not modelled: capacity and payloads of the observer's own making, under [Not modelled][not-modelled]. Rust tests in ShieldedLabs/zero#100: `tests/junk_flood.rs` fails on `main` with every junk entry held | Code read at ShieldedLabs/zero#100: `queue.rs:83`, `:283-291`, `:324`; `chain.rs`, `classify_send_response` and `classify_publish_failure`. zaino's relay: `zaino-source-zebra-rpc/src/adapter.rs`, `submission_rejection` |

On finding 2, the model lets the free-running clock be at most one flush interval ahead, so it can spend one epoch. `cadence_height` has no such cap. Reading that code, a clock further ahead would skip more than one boundary; the model does not exhibit that.

## How it is checked

> How much to rely on a green row, and how to produce one.

```sh
sh spec/quint/check.sh
```

"Holds" means one of three things, and the "Checked by" column says which:

- **TLC, exhaustive.** The hub specification. TLC visits every reachable state of the named configuration. The configurations are small (two or three payloads, a schedule scaled down from the shipped one, heights up to 12), so this is exhaustive for those parameters and not beyond them.
- **Exhaustive test.** `quint test` over a small finite universe: every input of a function, or every reachable hub state.
- **Simulation.** The protocol specification. `quint run`: at most 40 or 80 steps per trace, 2000 random traces, one seed. Not a proof and not exhaustive to any depth; a property that holds is one no sampled trace violated.

`quint verify` has not been run on any part of this. TLC is run through `tlc.sh`, because the compiled specification is larger than the Apalache server accepts.

| Tier | What | Expectation |
|---|---|---|
| 1 | `quint typecheck` on every file | ok |
| 2 | `quint test` on every test file | all pass, and each file reports at least the count `check.sh` gives it |
| 3 | `quint run`, invariants | "holds" rows hold; K2 and K10 are violated |
| 3b | `quint run`, witnesses | every listed state is reached at least once; no invariant is violated on the way |
| 4 | `tlc.sh`, one row per invariant and configuration | "holds" rows hold over every reachable state; "violated" rows are violated by a counterexample no longer than the recorded one |

Quint 0.33.0 is pinned (`npx --yes @informalsystems/quint@0.33.0` by default; set `QUINT=quint` to use an installed one). Tier 4 needs Java and Apalache 0.62.1, whose jar carries TLC; without either the tier fails, it never skips. `CHECK_TIERS=simulation` runs tiers 1 to 3b and `CHECK_TIERS=tlc` tiers 1 and 4; CI runs them as separate jobs. `CHECK_SHARD=I/N` runs every N-th row from row I, and CI splits tier 4 over three runners this way. All four tiers take about 10 minutes on a 16-core machine. It has not been timed on a CI runner.

### Configurations

A configuration is a value held in the state and selected by a named init, whose guard is that configuration's assumptions. A false guard leaves no initial state, and the gate fails.

#### Protocol specification

At most 3 sends and 3 lookups by the wallet, 3 requests by the observer. The `baseline` transactions are three Orchard migrations (one with an expiry below the supported floor), bytes nobody can parse, a Sapling payment, and the second leg of a TEX payment that one migration starts.

| Configuration | Hub | Indexer | Transactions |
|---|---|---|---|
| `baseline` | honest | honest | |
| `byzHub` | **Byzantine** | honest | |
| `byzIndexer` | honest | **Byzantine** | |
| `shieldedSpend` | honest | honest | and an auto-shield of a migration's output into Ironwood: K10 |

#### Hub specification

The schedule flushes every 3 blocks with a mining margin of 2, a delivery lag of 1, a reorg allowance of 1, a staleness window of 3 and an expiry floor of 10. It is the shipped schedule scaled down (interval 60, margin 12, lag 18, reorg allowance 10, staleness window 12 blocks, expiry floor 120). `staleLag` keeps the floor of 7 of the schedule before NU7. In `timely`, `flakyTip` and `flakyTipBudget` a block holds 3 units of shielded cost (shipped: 330); the other configurations set no block capacity and record nothing as mined. Other traffic takes 0 of a block's 3, or up to 1 in `flakyTipBudget`. Each migration costs 2, so the one block left after a flight takes one of `early` and `late`. `flakyTipBudget` sets a per-flush budget of 2.

| Configuration | Differs from `timely` by | For |
|---|---|---|
| `timely` | | G6b, G6c, K5 |
| `flakyTip` | the tip may be reported up to the reorg allowance behind | G6b under regression; G8, finding 6; K12 |
| `flakyTipBudget` | as `flakyTip`, with the candidate fix of ShieldedLabs/zero#121: a per-flush budget of 2 and a holdoff after each relay; other traffic takes up to 1 of each block | G6c, G6b and G8 under the fix |
| `flakyTipNoSlack` | and the expiry floor is 6, leaving no reorg slack | K3' |
| `staleLag` | the hub may hear no tip, and goes stale; the expiry floor is 7, as before NU7 | K4, K6 |
| `staleLagWithSlack` | and the expiry floor is 8, enough to cover the silence | finding 2 |
| `flakyStaleLagWithSlack` | as `staleLagWithSlack`, and the tip may also be reported up to the reorg allowance behind | K11 |
| `byzIndexer` | a Byzantine indexer, and a tight-expiry payload, at the expiry floor of 7 before NU7 | the trust matrix |
| `unknownUpgrade` | an unparseable payload; scripted runs only | the attempt bound |

In every configuration a due flush begins before the next block, and at most one block arrives while a flush is in flight. Under `staleLag` a hub is stale once it has seen no new high for the staleness window; its free-running clock is then assumed never behind the chain and at most one flush interval ahead of it. The implementation relies on "never behind" and does not enforce it (`hub/src/batcher.rs:86-89`).

<details>
<summary>What TLC visits</summary>

With 8 workers, on the machine this was written on:

| Configuration | Invariant | Verdict |
|---|---|---|
| `timely` | G6b | holds, 73 462 states, depth 43 |
| `timely` | K6's predicate | holds, 73 462 states, depth 43 |
| `flakyTip` | G6b | holds, 349 377 states, depth 42 |
| `timely`; `flakyTip`; `staleLagWithSlack` | G8 | holds: 73 462; 349 377; 895 083 states |
| `timely` | G6c | holds, 73 462 states, depth 43 |
| `flakyTip` | G6c (K12) | violated, 20 states |
| `flakyTipBudget` | G6c; `withinBudget`; G6b; G8 | holds, 201 421 states, depth 44 |
| `flakyTipNoSlack` | G6b (K3') | violated, 12 states |
| `staleLag` | G6b; K6 | violated, 13; 13 states |
| `staleLagWithSlack` | G6b (finding 2) | violated, 19 states |
| `flakyStaleLagWithSlack` | G6b (K11); G8 | violated, 15 states; holds, 2 461 236 states, not in the gate |
| `timely` | K5 | violated: 5 states; 8 without a crash; 15 without a shutdown either |
| `byzIndexer` | G6b | violated, 16 states |

Block capacity added the mined set and the miner's choice of block. `timely` and `flakyTip` model it and grew about 1.3 times. The other configurations set no block capacity and keep their state counts. Every old verdict and counterexample length stayed the same. The gate took about 10 minutes on a 16-core machine with 6 rows at once.

`flakyTip` had 81 518 states before the tip rule followed ShieldedLabs/zero#92. The hub's best height is now part of its state, so a tip one below the best and the same tip as the best are distinct states. Every verdict and counterexample length stayed the same.

The NU7 schedule raised the expiry floor of `timely` from 7 to 10. Every verdict stayed the same. Three counterexamples became 3 states longer: K12 (17 to 20), `wHeldOver` (17 to 20) and K5 without a shutdown (12 to 15). `byzIndexer` keeps the floor of 7, and its counterexample stays at 16 states.

TLC runs with deadlock checking off, so a machine whose steps had died would hold everything. The gate therefore also requires one reachable state per family of steps, and the antecedent of G6b, each as a violated `not(..)` row.

</details>

<details>
<summary>The abstraction lemma</summary>

The protocol specification's hub is the abstract one in `abstractHub.qnt`. `hubTest` checks, over every reachable state of the real hub function and every input, that each real step is a step of the abstract hub (`abstractionTest`), and that each abstract move has a real step behind it (`realisesTest`). So an invariant that holds over the abstract hub, and reads only queue membership and wire replies, holds over the real one: that covers G2, G3 and G4. It does not transfer reachability: the abstract hub answers where the real one is down or stale, so a violation shown over it is a state of the abstract hub. The reachable set is computed at a smaller schedule than the hub specification's and carried over by argument.

</details>

### Layout

Only `protocol.qnt` and `hubMachine.qnt` declare variables, and no module declares a constant. Every other module is pure.

| File | Owns |
|---|---|
| `spells/basicSpells.qnt`, `spells/soup.qnt` | `Option` and set and map helpers; the message soup |
| `types.qnt` | The vocabulary: payloads with their pool shape and parents, verdicts, refusals, roles, observations |
| `wire.qnt` | The four frames; `render`, `meaning`, `interpretReply` |
| `indexer.qnt` | The chain and indexer as a relation, honest and Byzantine |
| `hub.qnt` | `hub(state, input)`: admission, the tip rule, the flush cycle, requeue, the per-flush budget and the holdoff; the Byzantine relation |
| `shim.qnt` | `shim(state, input)`: the classifier, routing and reply correlation |
| `hubMachine.qnt` | The hub specification: one hub, the chain, its configurations and invariants |
| `abstractHub.qnt` | The hub as the protocol sees it |
| `protocol.qnt` | The protocol specification: state, steps, guarantees, gaps, witnesses, configurations |
| `tests/wireTest.qnt`, `indexerTest.qnt`, `shimTest.qnt`, `hubTest.qnt` | The functional properties; in `hubTest.qnt` also G7, A2, A3 and the abstraction lemma |
| `tests/hubScenariosTest.qnt` | Scripted runs of the hub specification: one per gap and per trust-matrix cell |
| `tests/scenariosTest.qnt`, `tests/trustTest.qnt` | Scripted runs of the protocol specification: gaps and witnesses; trust-matrix cells |
| `check.sh`, `tlc.sh` | The gate; one TLC check of one invariant on one configuration |

## Future work

> What would make these results bind the code.

1. **Failing Rust tests for the findings.** Findings 2, 6 and 8 have Rust tests, written with their fixes in ShieldedLabs/zero#85; the part of finding 2 that ShieldedLabs/zero#85 leaves has none. The other findings are shown on the model and matched to the code by reading. Finding 1 fits `batcher.rs`'s unit tests as 2 and 6 did. Finding 4 is impractical to reproduce.
2. **Model-based testing with `quint-connect`.** Depends on 1, and needs driver code in Rust. The specification is shaped for it: every branch of a step is a named action with its choices as named picks; each step gives one input to one component function and applies one output; and each function maps onto one seam in the Rust code.
3. **`quint verify` with Apalache for a subset of the claims.** The compiled specification is too large for the Apalache server today. A subset small enough to pass would give bounded symbolic checking of the protocol guarantees, which are simulated only.
4. **A Byzantine and a leaky shim.** Privacy properties are __hyperproperties__ and Quint doesn't support them (see the [discussion on the Quint Telegram channel][quint-tg]).

[protocol-spec]: ./protocol.qnt
[hub-spec]: ./hubMachine.qnt
[quint]: https://quint-lang.org
[system]: #system
[assumptions]: #assumptions
[out-of-scope]: #out-of-scope
[threat-model]: #threat-model
[guarantees]: #guarantees
[trust-matrix]: #trust-matrix
[known-gaps]: #known-gaps
[findings]: #findings
[how-it-is-checked]: #how-it-is-checked
[future-work]: #future-work
[zeronym-readme]: ../../README.md
[configurations]: #configurations
[wallet-threat-model]: https://zcash.readthedocs.io/en/latest/rtd_pages/wallet_threat_model.html
[not-modelled]: #not-modelled
[quint-tg]: https://t.me/quint_lang/1311
