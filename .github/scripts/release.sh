#!/usr/bin/env bash
# Release helpers for RELEASING.md.
#
# Usage:
#   .github/scripts/release.sh check-reproduce <sha>
#   .github/scripts/release.sh dispatch <workflow> <ref> [gh workflow run flags...]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

usage() { sed -n '4,6p' "$0" | sed 's/^# //' >&2; exit 2; }

# Pass when each zero-indexer reproduce workflow is green for <sha>. For each
# workflow, find the last commit at or before <sha> that touches its own
# on.push.paths, read from the workflow file at <sha>. Accept a green push run
# on that commit, or a green workflow_dispatch run on <sha>.
check_reproduce() {
  local sha wf paths c push dispatch fail=0
  sha=$(git rev-parse --verify "${1:?sha}^{commit}")
  for wf in zero-indexer-hub-reproduce.yml zero-indexer-shim-reproduce.yml; do
    # YAML 1.1 reads the bare key `on` as true, so look up both.
    paths=$(git show "$sha:.github/workflows/$wf" | ruby -ryaml -e '
      y = YAML.safe_load($stdin.read)
      paths = (y["on"] || y[true]).dig("push", "paths") or abort "no on.push.paths"
      abort "negated paths are unsupported" if paths.any? { |p| p.start_with?("!") }
      puts paths')
    local pathspecs=()
    while IFS= read -r p; do pathspecs+=(":(glob)$p"); done <<< "$paths"

    c=$(git log -1 --format=%H "$sha" -- "${pathspecs[@]}")
    push=$(gh run list --workflow "$wf" --commit "$c" --event push --status success --json databaseId --jq length)
    dispatch=$(gh run list --workflow "$wf" --commit "$sha" --event workflow_dispatch --status success --json databaseId --jq length)
    if (( push + dispatch > 0 )); then
      echo "$wf ok (${c:0:10})"
    else
      echo "$wf STOP (${c:0:10})"
      fail=1
    fi
  done
  return "$fail"
}

# Dispatch <workflow> on <ref>, wait for the run this dispatch created, and
# watch it. Exit non-zero when the run fails. Print the run's headSha.
dispatch() {
  local wf=${1:?workflow} ref=${2:?ref} head seen id
  shift 2
  head=$(gh api "repos/{owner}/{repo}/commits/$ref" --jq .sha)
  # The dispatch returns no run id, so the new run is the one absent before it.
  seen=$(gh run list --workflow "$wf" --commit "$head" --event workflow_dispatch \
           --limit 100 --json databaseId --jq '[.[].databaseId] | join(",")')
  gh workflow run "$wf" --ref "$ref" "$@"
  # A failing gh call exits through set -e; an empty result means "not yet".
  for _ in $(seq 60); do
    id=$(gh run list --workflow "$wf" --commit "$head" --event workflow_dispatch \
           --limit 100 --json databaseId --jq "[.[].databaseId] - [$seen] | .[0] // empty")
    [ -n "$id" ] && break
    sleep 5
  done
  [ -n "$id" ] || { echo "no new $wf run for $ref after 5 minutes" >&2; exit 1; }
  gh run watch "$id" --exit-status
  gh run view "$id" --json headSha --jq '"headSha \(.headSha)"'
}

case "${1:-}" in
  check-reproduce) shift; check_reproduce "$@" ;;
  dispatch) shift; dispatch "$@" ;;
  *) usage ;;
esac
