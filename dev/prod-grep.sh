#!/usr/bin/env bash
# Pre-commit content gate: the staged files must read as release material.
# Exit 1 on any hit. Allowed by construction: ISO timestamps as test data
# (the lookahead), the Date: field of an ADR header, RFC 7234 warn-agent,
# User-Agent, and a database/HTTP session in its technical sense.
set -u
mapfile -t files < <(git diff --cached --name-only --diff-filter=AM \
  | grep -vE '^(tasks\.md|antares-audit-tasks\.md|claude\.md|dev/prod-grep\.sh|target/|ngsi-ld-test-suite/|docs/spec/)' \
  | grep -vE '\.lock$')
[ "${#files[@]}" -eq 0 ] && { echo "prod-grep: nothing staged"; exit 0; }
pat='\bAI\b|\bagents?\b|Claude|MemPalace|subagent|scratchpad|workspace/docs|2026-[0-9]{2}-[0-9]{2}(?!T)|\bPhase [A-Z]\b|D-docs|closed-no-adopt|work-item|backlog|user (rule|request)|seen live|(this|earlier|previous|the) session\b|session (log|transcript|note)'
hits=$(grep -nHP "$pat" -- "${files[@]}" 2>/dev/null | grep -vE 'warn-agent|User-Agent|^docs/adr/ADR-[0-9]+[^:]*:3:Date:')
if [ -n "$hits" ]; then
  printf '%s\n' "$hits"
  echo "prod-grep: hits above block the commit"
  exit 1
fi
echo "prod-grep: clean (${#files[@]} files)"
