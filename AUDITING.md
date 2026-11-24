# Auditing an endpoint

An auditor can check a `zero-indexer` endpoint while treating its operator as untrusted. This is the full procedure. Each component guide has a link to it. Do all five steps on the shim, then on each hub that the shim lists. What a passing endpoint protects, and what stays exposed, are in the [threat model](./THREAT-MODEL.md).

Caddy ends TLS for `/attestation` and `/nym-address`. It is outside `EXPECTED_SHA256`, so it is trusted in steps 2 and 4.

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
