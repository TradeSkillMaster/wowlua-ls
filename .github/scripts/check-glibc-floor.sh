#!/usr/bin/env bash
# Fail if a Linux binary needs a newer glibc than FLOOR.
#
# Usage: check-glibc-floor.sh FLOOR BINARY...
#
# The dynamic loader refuses to start a binary that references a glibc symbol
# version its libc lacks ("version `GLIBC_2.38' not found"), so the newest
# GLIBC_* version in a binary's version needs is the oldest glibc it runs on.
# readelf reads any ELF architecture, so this also checks cross-built binaries.
set -euo pipefail

if [[ $# -lt 2 || -z $1 ]]; then
  echo "usage: $0 FLOOR BINARY... (no glibc floor given for this target?)" >&2
  exit 2
fi
floor=$1
shift

status=0
for bin in "$@"; do
  versions=$(readelf --version-info --wide "$bin")
  needed=$(grep -o 'Name: GLIBC_[0-9][0-9.]*' <<<"$versions" | sed 's/^Name: GLIBC_//' | sort -uV || true)
  if [[ -z $needed ]]; then
    # A glibc binary always needs some GLIBC_* version; finding none means the
    # binary isn't one (static, musl) or readelf's output changed, and either
    # way this check would be passing without having checked anything.
    echo "::error::$bin: no GLIBC_* version needs found" >&2
    status=1
    continue
  fi
  newest=$(tail -n 1 <<<"$needed")
  echo "$bin: needs glibc $newest (floor $floor)"
  while read -r version; do
    if [[ $(printf '%s\n' "$floor" "$version" | sort -V | tail -n 1) != "$floor" ]]; then
      symbols=$(readelf --dyn-syms --wide "$bin" | grep -o "[^ ]*@GLIBC_${version//./\\.}\b" | sort -u | tr '\n' ' ' || true)
      echo "::error::$bin needs GLIBC_$version, newer than the glibc $floor floor: $symbols" >&2
      status=1
    fi
  done <<<"$needed"
done
exit "$status"
