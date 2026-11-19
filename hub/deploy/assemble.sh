#!/bin/sh
# Assemble the zero-indexer-hub build context: a partial mirror of the zero repo
# containing the hub plus the vendor/ crates its [patch] sections name.
# It also carries zero-indexer-proto, the path crate both binaries share.
# zebra-chain comes from crates.io, pinned by the committed Cargo.lock. Sibling of shim/deploy/assemble.sh, with the same shape.
#
# Everything comes out of `git archive HEAD`, never the working tree. Three
# reasons, all of them load-bearing:
#   1. Provenance: the context is exactly the recorded SHA, so "rebuild from
#      source" has an unambiguous meaning. That includes the Containerfile,
#      which IS the build; see the recipe-provenance block near the bottom.
#   2. The working copy stays untouched, so a build leaves the checkout clean.
#   3. Determinism: git archive stamps every entry with the commit timestamp,
#      so file mtimes in the context are identical on every machine.
#
# No network access is required.
#
# PORTABILITY: this is POSIX sh, deliberately. Callers invoke it as
# `sh assemble.sh`, which bypasses any shebang, and on Debian and Ubuntu /bin/sh
# is dash. dash has no `-o pipefail`, so this uses `set -eu` and no pipelines:
# each `git archive` writes a tar file whose exit status `set -e` actually
# checks. Do not reintroduce a pipeline here without pipefail, and do not
# reintroduce pipefail.
#
# Usage: sh hub/deploy/assemble.sh [dest-dir]

set -eu

# Context file modes must not depend on who runs the script (see the shim's
# assemble.sh for the full argument): git archive records the committed mode but
# `tar -x` masks it with the invoking user's umask. Belt and braces: umask here,
# `-p` on every extraction below.
umask 022

ZERO_ROOT=$(git rev-parse --show-toplevel)
HERE="$ZERO_ROOT/hub/deploy"
DEST=${1:-"$(dirname "$ZERO_ROOT")/zero-indexer-hub-ctx"}
SHA=$(git -C "$ZERO_ROOT" rev-parse --short HEAD)

echo "==> assembling zero-indexer-hub context from zero@$SHA into $DEST"
rm -rf "$DEST"
mkdir -p "$DEST"

STAGE=$(mktemp -d)
# shellcheck disable=SC2064
trap "rm -rf '$STAGE'" EXIT INT TERM

# The crate. Its manifest names ../vendor/* in its [patch] sections, so the
# context keeps hub/ and vendor/ as siblings at its root and the Containerfile
# reconstructs them under /usr/src/app. No manifest is edited, here or in the
# image. The archive reads the commit HEAD, so every entry carries the
# committer time.
#
# This also carries deploy/ itself, which is how the build gets a commit-pinned
# copy of its own Containerfile.
git -C "$ZERO_ROOT" archive HEAD -o "$STAGE/hub.tar" hub
tar -xpf "$STAGE/hub.tar" -C "$DEST"

# The crypto-common [patch] target. hub/Cargo.toml unconditionally patches nym's
# nym-upgrade-mode-check to this vendored slim copy, so cargo must find it to even
# PARSE the manifest, whether or not the mixnet-driver feature is on. The patch
# only BINDS when nym-sdk is in the graph (that feature), but the path has to
# exist regardless, so this is not optional. Keep it in the reproduce workflow's
# `paths:` filter alongside the crate itself. The two webpki-roots copies are
# [patch.crates-io] targets under the same rule.
git -C "$ZERO_ROOT" archive HEAD -o "$STAGE/vendor.tar" \
	vendor/nym-upgrade-mode-check \
	vendor/webpki-roots-0.24.0 \
	vendor/webpki-roots-0.25.4
tar -xpf "$STAGE/vendor.tar" -C "$DEST"

# zero-indexer-proto, which hub/Cargo.toml names as `../proto`. Only the manifest
# and the committed generated code go in: the generator under proto/gen/ and
# the .proto sources take no part in the build.
git -C "$ZERO_ROOT" archive HEAD -o "$STAGE/proto.tar" \
	proto/Cargo.toml \
	proto/src
tar -xpf "$STAGE/proto.tar" -C "$DEST"

# Also not needed: a rust-toolchain.toml. The hub's own lockfile is
# authoritative, and the pinned pallet-rust digest is the toolchain.

############################################################
# Recipe provenance
############################################################
# The Containerfile is the entire definition of the build, so it is the one file
# in the context most worth tampering with, and therefore the one that must come
# from HEAD rather than from the working tree. It arrives via the `git archive
# HEAD hub` above, at $DEST/hub/deploy/Containerfile, and every
# caller builds with `-f` pointed at THAT path. There is deliberately no copy at
# the context root: a second, working-tree-sourced copy is exactly the hole this
# closes.
RECIPE="$DEST/hub/deploy/Containerfile"

if [ -f "$RECIPE" ]; then
	# Committed. Warn if the working tree has drifted, because the build will
	# silently use HEAD's version and someone iterating on the recipe needs to
	# know why their edit had no effect.
	if ! cmp -s "$HERE/Containerfile" "$RECIPE"; then
		echo "note: your working-tree Containerfile differs from HEAD's."
		echo "      The build uses HEAD's copy, by design. Commit to test edits."
	fi
else
	# Not committed yet. Fall back to the working tree so the recipe can be
	# developed at all, but say so loudly: a hash produced this way is not
	# pinned to a commit and must not be published as one.
	echo
	echo "############################################################"
	echo "# WARNING: hub/deploy/ is NOT COMMITTED at $SHA."
	echo "# Falling back to the working-tree copy of deploy/."
	echo "# The context is therefore NOT fully commit-pinned, and a hash"
	echo "# produced from it is not reproducible by a third party, who"
	echo "# would check out $SHA and find no deploy/ at all."
	echo "# Commit hub/deploy/ before publishing any hash."
	echo "############################################################"
	echo
	mkdir -p "$DEST/hub/deploy"
	cp "$HERE/Containerfile" "$RECIPE"
	# Mirror the rest of deploy/ too, so the context has the same shape it will
	# have once committed. None of it is compiled (cargo builds src/ and the
	# manifest; deploy/ is inert), which is what makes the recorded hash survive
	# committing this directory.
	for f in "$HERE"/*; do
		[ -f "$f" ] || continue
		cp "$f" "$DEST/hub/deploy/"
	done
fi

echo "==> assembled: $DEST ($(du -sh "$DEST" | cut -f1))"
echo "recipe: $RECIPE"
echo "verify: docker build --platform linux/amd64 -f $RECIPE $DEST"
