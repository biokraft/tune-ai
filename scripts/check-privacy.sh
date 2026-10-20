#!/usr/bin/env bash
# Blocks audio files and personal/machine-specific data from being committed.
#   check-privacy.sh            scan all tracked files
#   check-privacy.sh --staged   scan files staged for commit (index content)
# Exit 0 when clean, 1 with one "path: reason" line per finding.
# Portable on purpose: bash + git + grep only.
set -u

cd "$(git rev-parse --show-toplevel)" || exit 2

STAGED=0
[ "${1:-}" = "--staged" ] && STAGED=1

ALLOWLIST="scripts/privacy-allowlist.txt"
AUDIO_RE='\.(wav|mp3|flac|aif|aiff|ogg|m4a)$'
# Home-directory prefixes, assembled so this file never holds them literally.
HOME_RE='(/Us''ers/|/ho''me/)[A-Za-z0-9._-]+'
EMAIL_RE='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'
EMAIL_OK_RE='(noreply@anthropic\.com|@example\.(com|org|net))$'
COPYRIGHT_RE='[Cc]opyright[[:space:]]*(\(c\)|©)?[[:space:]]*[0-9]{4}[[:space:]]+[A-Za-z]'

is_allowlisted() {
  [ -f "$ALLOWLIST" ] || return 1
  grep -qxF -- "$1" "$ALLOWLIST"
}

if [ "$STAGED" -eq 1 ]; then
  FILES="$(git diff --cached --name-only --diff-filter=ACM)"
else
  FILES="$(git ls-files)"
fi

# Configured git identity must never appear in files (unless allowlisted).
IDENTITY=""
for v in user.name user.email; do
  val="$(git config "$v" 2>/dev/null || true)"
  [ "${#val}" -ge 4 ] && IDENTITY="$IDENTITY$val"$'\n'
done

FOUND=0
report() { echo "$1: $2"; FOUND=1; }

content() {
  if [ "$STAGED" -eq 1 ]; then git show ":$1" 2>/dev/null; else cat -- "$1" 2>/dev/null; fi
}

while IFS= read -r f; do
  [ -n "$f" ] || continue

  if printf '%s' "$f" | grep -qiE "$AUDIO_RE"; then
    report "$f" "audio file"
    continue
  fi

  allow=0
  is_allowlisted "$f" && allow=1

  body="$(content "$f" | grep -aI '' || true)"
  [ -n "$body" ] || continue

  if printf '%s\n' "$body" | grep -qE "$HOME_RE"; then
    report "$f" "home path"
  fi

  if printf '%s\n' "$body" | grep -oE "$EMAIL_RE" | grep -qvE "$EMAIL_OK_RE"; then
    report "$f" "email address"
  fi

  if [ "$allow" -eq 0 ]; then
    if printf '%s\n' "$body" | grep -qE "$COPYRIGHT_RE"; then
      report "$f" "copyright name line (add to $ALLOWLIST if it is a license file)"
    fi
    if [ -n "$IDENTITY" ]; then
      while IFS= read -r id; do
        [ -n "$id" ] || continue
        if printf '%s\n' "$body" | grep -qiF -- "$id"; then
          report "$f" "configured git identity"
          break
        fi
      done <<< "$IDENTITY"
    fi
  fi
done <<< "$FILES"

exit "$FOUND"
