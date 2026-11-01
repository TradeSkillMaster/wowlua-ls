#!/usr/bin/env bash
# Prints the path of a `local`-profile wowlua_ls binary built from a git revision
# (default: main), for the "before" side of a before/after comparison:
#
#   base=$(tests/baseline.sh "$(git merge-base HEAD main)")
#   "$base" check /path/to/addon > before.txt
#
# Binaries are cached per commit and shared by every worktree. Builds take turns in
# one scratch tree and target dir, so a new commit only rebuilds what it changed.
set -euo pipefail

sha=$(git rev-parse --verify "${1:-main}^{commit}")
root=${WOWLUA_BASELINE_DIR:-/var/tmp/wowlua-baseline}
bin=$root/bin/$sha/wowlua_ls

if [ ! -x "$bin" ]; then
    mkdir -p "$root/tree"
    exec 9>"$root/lock"
    flock 9
    if [ ! -x "$bin" ]; then
        # Through a private index, only the files that differ from the last build are
        # rewritten, so only those look changed to cargo. Checkout smudges the LFS stubs.
        GIT_INDEX_FILE=$root/index git --work-tree="$root/tree" read-tree -u --reset "$sha"
        # Commits from before the `local` profile existed get it on the command line.
        profile_def=()
        grep -q '^\[profile\.local\]' "$root/tree/Cargo.toml" || profile_def=(
            --config 'profile.local.inherits="release"' --config profile.local.lto=false
            --config profile.local.incremental=true --config profile.local.codegen-units=256)
        env -u RUSTUP_TOOLCHAIN CARGO_TARGET_DIR="$root/target" cargo build --locked \
            --profile local "${profile_def[@]}" --manifest-path "$root/tree/Cargo.toml" >&2
        mkdir -p "${bin%/*}"
        cp "$root/target/local/wowlua_ls" "$bin.tmp"
        mv "$bin.tmp" "$bin"
        # Drop binaries nobody has asked for in two weeks.
        find "$root/bin" -name wowlua_ls -mtime +14 -delete
        find "$root/bin" -mindepth 1 -type d -empty -delete
    fi
fi
touch -c "$bin"
echo "$bin"
