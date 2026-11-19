# Releasing zeronym

This is the playbook for a zeronym release. Run the steps below on cut day, in order.

> [!IMPORTANT]
> Versions are `vX.Y.Z` tags. One tag covers the whole repository. In every command below, replace `vX.Y.Z` with the new version (for example `v0.1.0`).

## 1. Cut the changelog

Notes collect under `## Unreleased` in `CHANGELOG.md`. Land the cut through one PR on a `release-prep-vX.Y.Z` branch.

1. Rename `## Unreleased` to `## vX.Y.Z - YYYY-MM-DD`.
2. Add a new empty `## Unreleased` above it.
3. Open the PR and merge it.

```sh
git switch main && git pull
git switch -c release-prep-vX.Y.Z
# edit CHANGELOG.md
git commit -am "chore(release): cut vX.Y.Z"
git push -u origin HEAD
gh pr create --draft --base main --title "chore(release): cut vX.Y.Z" --fill
```

## 2. Pin the release commit

The release is exactly the merge commit of the prep PR. Later commits on `main` stay out of it. Push a `release-vX.Y.Z` branch at that commit. The chain stops when the lookup comes back empty, because `git push origin ":ref"` with an empty SHA deletes the remote branch. GitHub dispatches a workflow on a branch or tag name, so the branch carries the SHA.

```sh
git fetch origin
SHA=$(gh pr list --state merged --head release-prep-vX.Y.Z --json mergeCommit --jq '.[0].mergeCommit.oid')
test -n "$SHA" &&
git log -1 --oneline "$SHA" &&
git push origin "$SHA:refs/heads/release-vX.Y.Z"
```

Delete `release-vX.Y.Z` after the release is out.

## 3. Check the builds

A red reproduce run blocks nothing on its own, so check both builds for the release commit. For each workflow, the script reads the push paths from the workflow file at `$SHA` and finds the last commit at or before `$SHA` that touches them. It then looks for a green run that proves that commit.

- A green `push` run on that commit counts.
- A green `workflow_dispatch` run on `$SHA` also counts. Use it when the push run was cancelled by a later push.

```sh
.github/scripts/release.sh check-reproduce "$SHA"
```

Both lines must say `ok`. On `STOP`, dispatch that workflow on `release-vX.Y.Z` and run the check again once it passes.

```sh
.github/scripts/release.sh dispatch zeronym-hub-reproduce.yml release-vX.Y.Z   # or the shim one
```

## 4. Tag and release

The release body is the changelog section plus the reproducible-build pins from the two `EXPECTED_SHA256` files. A live enclave's hash lives in `PROVENANCE` on its `deploy-<app-id>` tag.

```sh
hub=$(git show "$SHA:hub/deploy/EXPECTED_SHA256")
shim=$(git show "$SHA:shim/deploy/EXPECTED_SHA256")
git show "$SHA:CHANGELOG.md" | awk -v tag="vX.Y.Z" '
  $0 ~ "^## "tag"([[:space:]]|$)" {on=1; next}
  /^## / {on=0}
  on' > changes.md
cp changes.md notes.md
cat >> notes.md <<EOF

## Reproducible-build pins

These are the reproducible-build pins at [vX.Y.Z](https://github.com/ShieldedLabs/zeronym/tree/vX.Y.Z) (\`$SHA\`).

- Hub \`$hub\` ([EXPECTED_SHA256](https://github.com/ShieldedLabs/zeronym/blob/$SHA/hub/deploy/EXPECTED_SHA256))
- Shim \`$shim\` ([EXPECTED_SHA256](https://github.com/ShieldedLabs/zeronym/blob/$SHA/shim/deploy/EXPECTED_SHA256))

A live enclave runs the hash in \`PROVENANCE\` on its \`deploy-<app-id>\` tag. Check that file to see what a given endpoint runs.
EOF
```

Read `notes.md`. It must start with the full changelog section. Then tag `$SHA` and create the release. The chain stops at the first command that fails, and the first one fails when `CHANGELOG.md` has no `vX.Y.Z` section at `$SHA`.

```sh
grep -q '[^[:space:]]' changes.md &&
git tag -a vX.Y.Z "$SHA" -m "zeronym vX.Y.Z" &&
git push origin vX.Y.Z &&
gh release create vX.Y.Z --verify-tag --title "zeronym vX.Y.Z" --notes-file notes.md
```

## 5. Announce

**Forum and X.** Post the release on the [Zcash Community Forum](https://forum.zcashcommunity.com/) and on X. Add one sentence on rollout order and any new required flags. Point readers at the [hub operator guide](hub/deploy/caution/OPERATORS.md) and the [shim operator guide](shim/deploy/caution/OPERATORS.md).

**Dashboard.** Notify the [indexer dashboard](https://zecblock.com/network/attestations) maintainers. Send them the tag, the commit and both `EXPECTED_SHA256` values at that commit.
