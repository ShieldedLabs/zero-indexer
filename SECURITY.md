# Security policy

Report vulnerabilities in `zero-indexer` through the Shielded Labs security disclosure group on Signal. The rest of the Zero distribution uses the same channel.

## Report a vulnerability

1. Join the group: <https://signal.group/#CjQKICZtmwnx-qJlNzqu9ACZno_s9hMZhELfjod-KBGXVXxUEhA-p8Ai5BgwAVVllZvDV6tb>
2. The group is a triage waiting room. After admission, say only that you have a report. Keep the details out of the waiting room.
3. Shielded Labs moves you into a private group with the relevant people. Disclose the details there.
4. Shielded Labs then removes you from the waiting room.

Keep vulnerability details out of public issues, pull requests and discussions.

## Scope

In scope:

- The shim (`shim/`).
- The hub (`hub/`).
- The protocol between the shim and the hub: the messages in `proto/` and the model in `spec/`.
- The live attested endpoints: the shim and hub enclaves on Caution. This includes the build and attestation claims in the [auditor procedure](./README.md#usage).

Out of scope:

- Wallet-local concerns. In the [wallet app threat model](https://zcash.readthedocs.io/en/latest/rtd_pages/wallet_threat_model.html), these belong to the wallet. Report them to the wallet's maintainers.
- Third-party indexers, including the indexer behind a shim. Report these to their maintainers.

The threat model and the known limitations are in the [Security](./README.md#security) section of the README.

## Embargo and response times

The embargo length and the response times will be in the Shielded Labs disclosure policy. That policy is a draft. This file will link to it after publication.
