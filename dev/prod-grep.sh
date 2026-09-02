#!/usr/bin/env bash
# Content gate: the files must read as release material. Exit 1 on any hit.
# Default subject is what is staged; `--all` runs over every tracked file.
# Allowed by construction: ISO timestamps as test data (the lookahead), the
# Date: field of an ADR header, RFC 7234 warn-agent, User-Agent, and a
# database/HTTP session in its technical sense. A run number of this repo's
# own CI resolves to nothing a reader of the source can reach, so it is a
# hit; an issue of another project (Scorpio, ETSI TC DATA) is a citation
# and stays.
#
# GNU grep by absolute path: a `grep` on PATH may be ugrep, which skips
# files it decides are binary without saying so — a gate that silently
# passes by not reading is worse than no gate.
set -euo pipefail
GREP=/usr/bin/grep

usage() { echo "usage: $0 [--all]"; exit 2; }
subject=staged
case "${1-}" in
  --all) subject=all ;;
  "") ;;
  *) usage ;;
esac

# CHANGELOG.md is dated by definition; lockfiles are generated.
skip='^(tasks\.md|antares-audit-tasks\.md|claude\.md|CHANGELOG\.md|dev/prod-grep\.sh|target/|ngsi-ld-test-suite/|docs/spec/)'
if [ "$subject" = all ]; then
  mapfile -t files < <(git ls-files | { "$GREP" -vE "$skip" || true; } | { "$GREP" -vE '(\.lock|-lock\.json|\.pdf|\.png|\.jpg|\.gif|\.woff2?|\.ico)$' || true; })
else
  mapfile -t files < <(git diff --cached --name-only --diff-filter=AM \
    | { "$GREP" -vE "$skip" || true; } | { "$GREP" -vE '\.lock$' || true; })
fi
[ "${#files[@]}" -eq 0 ] && { echo "prod-grep: nothing to check"; exit 0; }

pat='\bAI\b|\bagents?\b|Claude|MemPalace|subagent|scratchpad|workspace/docs|2026-[0-9]{2}-[0-9]{2}(?!T)|\bPhase [A-Z]\b|D-docs|closed-no-adopt|work-item|backlog|user (rule|request)|seen live|(this|earlier|previous|the) session\b|session (log|transcript|note)|\b(CI|run) #[0-9]+'
allow='warn-agent|User-Agent|^docs/adr/ADR-[0-9]+[^:]*:3:Date:'
# Both greps exit 1 on no match, which under pipefail would kill the script
# on exactly the clean input it is meant to pass.
hits=$({ "$GREP" -nHP "$pat" -- "${files[@]}" 2>/dev/null || true; } \
  | { "$GREP" -vE "$allow" || true; })
if [ -n "$hits" ]; then
  printf '%s\n' "$hits"
  echo "prod-grep: hits above block the commit"
  exit 1
fi
echo "prod-grep: clean (${#files[@]} files, $subject)"
