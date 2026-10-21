#!/usr/bin/env bash
# Tests for scripts/check-privacy.sh. Sensitive strings are built by
# concatenation so this file never contains them literally.
set -u

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-privacy.sh"
FAILS=0

U1="/Us""ers/"
H1="/ho""me/"
AT="@"

new_repo() {
  T="$(mktemp -d)"
  git -C "$T" init -q
  mkdir -p "$T/scripts"
  cp "$SRC" "$T/scripts/check-privacy.sh"
  printf '%s\n' "LICENSE" > "$T/scripts/privacy-allowlist.txt"
}

# expect <name> <want-exit> <want-substring-or-empty>
run_case() {
  local name="$1" want="$2" sub="$3" out rc
  out="$(cd "$T" && bash scripts/check-privacy.sh --staged 2>&1)"
  rc=$?
  if [ "$rc" -ne "$want" ] || { [ -n "$sub" ] && ! printf '%s' "$out" | grep -qF -- "$sub"; }; then
    echo "FAIL: $name (exit $rc, want $want, want substring '$sub')"
    echo "$out" | sed 's/^/    /'
    FAILS=$((FAILS + 1))
  else
    echo "ok: $name"
  fi
  rm -rf "$T"
}

if [ ! -f "$SRC" ]; then
  echo "FAIL: $SRC missing"
  exit 1
fi

for ext in wav mp3 flac aif aiff ogg m4a WAV Mp3; do
  new_repo
  : > "$T/x.$ext"
  git -C "$T" add -f "x.$ext"
  run_case "audio .$ext" 1 "x.$ext: audio file"
done

for name in x.wav.tmp x.flac.tmp; do
  new_repo
  : > "$T/$name"
  git -C "$T" add -f "$name"
  run_case "audio temp $name" 1 "$name: audio file"
done

new_repo
echo "see ${U1}someone/file" > "$T/a.txt"; git -C "$T" add a.txt
run_case "users home path" 1 "a.txt: home path"

new_repo
echo "see ${H1}someone/file" > "$T/a.txt"; git -C "$T" add a.txt
run_case "linux home path" 1 "a.txt: home path"

new_repo
echo "mail someone${AT}gmail.com" > "$T/a.txt"; git -C "$T" add a.txt
run_case "email" 1 "a.txt: email address"

new_repo
printf 'noreply%santhropic.com\nfoo%sexample.com\n' "$AT" "$AT" > "$T/a.txt"; git -C "$T" add a.txt
run_case "allowed emails" 0 ""

new_repo
echo "Copy""right (c) 2024 Some Name" > "$T/LICENSE"; git -C "$T" add LICENSE
run_case "allowlisted copyright line" 0 ""

new_repo
echo "fn main() {}" > "$T/a.rs"; git -C "$T" add a.rs
run_case "clean file" 0 ""

# Renames must still be scanned (rename detection must not hide them).
new_repo
echo "plain notes" > "$T/notes.txt"; git -C "$T" add notes.txt
git -C "$T" -c user.name=t -c user.email=t${AT}example.com commit -q -m t --no-verify
git -C "$T" mv notes.txt take.wav
run_case "rename to audio" 1 "take.wav: audio file"

new_repo
printf 'line one\nline two\nline three\nline four\n' > "$T/notes.txt"; git -C "$T" add notes.txt
git -C "$T" -c user.name=t -c user.email=t${AT}example.com commit -q -m t --no-verify
git -C "$T" mv notes.txt moved.txt
echo "mail someone${AT}gmail.com" >> "$T/moved.txt"; git -C "$T" add moved.txt
run_case "rename with email" 1 "moved.txt: email address"

# Non-ASCII paths: git quotes them unless listed NUL-delimited.
DASH="$(printf '\342\200\223')"
new_repo
: > "$T/Artist ${DASH} Track.mp3"; git -C "$T" add -f "Artist ${DASH} Track.mp3"
run_case "non-ascii audio filename (staged)" 1 "Track.mp3: audio file"

new_repo
echo "see ${U1}someone/file" > "$T/notes ${DASH} e.md"; git -C "$T" add "notes ${DASH} e.md"
run_case "non-ascii text file home path (staged)" 1 "e.md: home path"

new_repo
: > "$T/Artist ${DASH} Track.mp3"; git -C "$T" add -f "Artist ${DASH} Track.mp3"
git -C "$T" -c user.name=t -c user.email=t${AT}example.com commit -q -m t --no-verify
out="$(cd "$T" && bash scripts/check-privacy.sh 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -qF "Track.mp3: audio file"; then
  echo "ok: non-ascii audio filename (full scan)"
else
  echo "FAIL: non-ascii audio filename (full scan) (exit $rc)"; FAILS=$((FAILS + 1))
fi
rm -rf "$T"

new_repo
echo "see ${U1}someone/file" > "$T/notes ${DASH} e.md"; git -C "$T" add "notes ${DASH} e.md"
git -C "$T" -c user.name=t -c user.email=t${AT}example.com commit -q -m t --no-verify
out="$(cd "$T" && bash scripts/check-privacy.sh 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -qF "e.md: home path"; then
  echo "ok: non-ascii text file home path (full scan)"
else
  echo "FAIL: non-ascii text file home path (full scan) (exit $rc)"; FAILS=$((FAILS + 1))
fi
rm -rf "$T"

# Non-staged mode scans tracked files.
new_repo
echo "see ${U1}someone/file" > "$T/a.txt"; git -C "$T" add a.txt
git -C "$T" -c user.name=t -c user.email=t${AT}example.com commit -q -m t --no-verify
out="$(cd "$T" && bash scripts/check-privacy.sh 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -qF "a.txt: home path"; then
  echo "ok: full scan"
else
  echo "FAIL: full scan (exit $rc)"; FAILS=$((FAILS + 1))
fi
rm -rf "$T"

if [ "$FAILS" -ne 0 ]; then echo "$FAILS failure(s)"; exit 1; fi
echo "all privacy tests passed"
