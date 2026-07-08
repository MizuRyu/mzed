#!/usr/bin/env bash
# Pre-push gate: block the push when secrets, personal information, or a
# failing build would leave the machine. Wired via pre-commit's pre-push
# stage (`pre-commit install --hook-type pre-push`).
#
# Skip the (slow) verify step only: MZED_SKIP_VERIFY=1 git push
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
fail=0

echo "==> [1/3] gitleaks: scanning full git history..."
if command -v gitleaks >/dev/null 2>&1; then
  if ! gitleaks git --no-banner --exit-code 1 . ; then
    echo "error: gitleaks found potential secrets in the history" >&2
    fail=1
  fi
else
  echo "error: gitleaks is not installed (brew install gitleaks)" >&2
  fail=1
fi

echo "==> [2/3] personal info: scanning tracked files..."
# Absolute home paths (test fixtures use the fake user "me", which is allowed)
# and personal email providers. Generic patterns on purpose: the real values
# must not appear in this public script either.
paths_hit=$(git ls-files -z | xargs -0 grep -nE '/Users/[A-Za-z0-9._-]+' 2>/dev/null | grep -v '/Users/me[/"'"'"']' || true)
if [ -n "$paths_hit" ]; then
  echo "error: absolute /Users/ paths in tracked files:" >&2
  echo "$paths_hit" | head -10 >&2
  fail=1
fi
mail_hit=$(git ls-files -z | xargs -0 grep -nE '[A-Za-z0-9._%+-]+@(gmail|yahoo|outlook|icloud|hotmail)\.' 2>/dev/null || true)
if [ -n "$mail_hit" ]; then
  echo "error: personal email addresses in tracked files:" >&2
  echo "$mail_hit" | head -10 >&2
  fail=1
fi

if [ "${MZED_SKIP_VERIFY:-0}" = "1" ]; then
  echo "==> [3/3] verify: skipped (MZED_SKIP_VERIFY=1)"
else
  echo "==> [3/3] just verify (fmt + clippy + test)..."
  if ! just verify; then
    echo "error: just verify failed" >&2
    fail=1
  fi
fi

if [ "$fail" -ne 0 ]; then
  echo "" >&2
  echo "push blocked by scripts/check-push.sh" >&2
  exit 1
fi
echo "==> push gate: all clear"
