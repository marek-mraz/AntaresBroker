#!/usr/bin/env bash
# MAC-SIDE ONLY — needs an authenticated `gh` (the sandbox has no GitHub
# credentials). Sets up the merge-only branch model the workflows expect:
#
#   main — production. PR-only; the required check is full.yml (the whole
#          grid: workspace + all seven ETSI cells + both wasm tiers).
#          Release tags (v*) are cut here.
#   dev  — integration, DEFAULT branch (scheduled workflows and new PRs
#          land here). PR-only; the required check is ci.yml (workspace +
#          quick matrix). Every green merge publishes :dev.
#
# Direct pushes to either branch are rejected — work happens on feature
# branches, PRs carry it in. Review count is 0 on purpose: a solo
# maintainer cannot approve their own PR, so requiring one would deadlock.
#
# Run ONCE, from anywhere inside the repo clone. Idempotence: the rename
# and ref-create steps fail harmlessly if already done; rulesets error on a
# duplicate name — delete the old one first (gh api repos/…/rulesets).
set -euo pipefail

REPO=${REPO:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}
echo "repo: $REPO"

# 1. master -> main. GitHub retargets open PRs and web links itself, but
#    every clone must follow: git branch -m master main &&
#    git fetch origin && git branch -u origin/main main
gh api -X POST "repos/$REPO/branches/master/rename" -f new_name=main \
  && echo "renamed master -> main" \
  || echo "rename skipped (already main?)"

# 2. dev branched from main
main_sha=$(gh api "repos/$REPO/git/ref/heads/main" -q .object.sha)
gh api -X POST "repos/$REPO/git/refs" -f ref=refs/heads/dev -f sha="$main_sha" \
  && echo "created dev at $main_sha" \
  || echo "dev creation skipped (already exists?)"

# 3. dev is the default branch — scheduled workflows (full 2x/week, strict,
#    fuzz, advisories, etsi-coverage, roll-weekly, examples) run on the
#    default branch, and that must be where the churn is.
gh repo edit "$REPO" --default-branch dev
echo "default branch: dev"

# 4. rulesets — merge-only + required checks + no force-push/deletion.
#    Check contexts are "<caller job name> / <called job name>"; if GitHub
#    reports a check as expected-but-never-reported, copy the exact names
#    from a real PR's checks tab and update here.
ruleset() {
  local name=$1 branch=$2; shift 2
  local checks="" c
  for c in "$@"; do checks+="{\"context\":\"$c\"},"; done
  gh api -X POST "repos/$REPO/rulesets" --input - <<JSON && echo "ruleset $name active"
{
  "name": "$name",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/heads/$branch"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "pull_request", "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false } },
    { "type": "required_status_checks", "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [ ${checks%,} ] } }
  ]
}
JSON
}

ruleset protect-dev dev \
  "Workspace tests / Workspace tests" \
  "ETSI cell matrix / Matrix summary"

ruleset protect-main main \
  "Workspace tests / Workspace tests" \
  "ETSI cell matrix / Matrix summary" \
  "Browser build (wasm32)" \
  "wasm Node tier (serial suites)"

cat <<'EOF'

DONE. Remaining by hand:
 - Update the auto-pusher: it must push a WORK branch (e.g. sandbox-work)
   and open/refresh a PR into dev — direct pushes to dev/main are now
   rejected, and pushing the old "master" name would just create a stray
   branch.
 - Local clones: git branch -m master main; git fetch origin;
   git branch -u origin/main main; git remote set-head origin -a
EOF
