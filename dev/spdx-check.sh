#!/usr/bin/env sh
# Every Rust source file opens with the SPDX licence line; list the ones that
# do not and fail. `dev/spdx-check.sh fix` prepends the line instead.
# POSIX sh has no pipefail; the one pipeline below is guarded by hand.
set -eu
cd "$(dirname "$0")/.."
LINE='// SPDX-License-Identifier: EUPL-1.2'
missing=$(find crates -name '*.rs' -not -path '*/target/*' | while read -r f; do
  [ "$(head -n1 "$f")" = "$LINE" ] || echo "$f"
done)
[ -n "$missing" ] || { echo "spdx: every source file carries the licence line"; exit 0; }
if [ "${1:-}" = fix ]; then
  echo "$missing" | while read -r f; do printf '%s\n' "$LINE" | cat - "$f" > "$f.spdx" && mv "$f.spdx" "$f"; done
  echo "spdx: header added to $(echo "$missing" | wc -l) files"
else
  echo "$missing"; echo "spdx: files above lack '$LINE' (run dev/spdx-check.sh fix)"; exit 1
fi
