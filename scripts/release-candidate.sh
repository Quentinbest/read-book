#!/bin/sh
# Release acceptance (docs/release-acceptance.md): put a release candidate on this
# Mac for the owner's hand test. Downloads the draft's DMG and updater archive for
# this Mac, checks the archive against the update key as the updater does (D6) and
# the DMG's app against it, backs up the library folder and the installed app,
# installs the candidate and opens it. --restore puts a backup back.
# Nothing is deleted: whatever is replaced is moved aside.
#
#   scripts/release-candidate.sh v0.5.2              install the candidate
#   scripts/release-candidate.sh --restore <backup>  put that backup back
#
# LINEN_RC_APP and LINEN_RC_DATA point it at another app and library folder, and
# LINEN_RC_NO_OPEN=1 leaves the app closed (to try the script without touching the
# installed Linen).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO=Quentinbest/read-book
APP=${LINEN_RC_APP:-/Applications/Linen.app}
DATA=${LINEN_RC_DATA:-"$HOME/Library/Application Support/app.linen.reader"}
STAMP=$(date +%Y%m%d-%H%M%S)

fail() { echo "release-candidate: $*" >&2; exit 1; }
# Any Linen with this library open (whichever copy of the app it runs from), or one
# running from the app about to be replaced. The library must not change while it is
# copied or moved.
running() {
  pgrep -qf "$APP/Contents/MacOS/" && return 0
  for f in "$DATA/linen.db" "$DATA/linen.db-wal"; do
    if [ -e "$f" ] && lsof -t -- "$f" > /dev/null 2>&1; then return 0; fi
  done
  return 1
}
# Moves $1 aside into $2 (when it exists), then puts the backed-up copy $3 in its
# place, or leaves it absent when the backup recorded that it was absent ($4).
put_back() {
  if [ -e "$1" ]; then mv "$1" "$2"; fi
  if [ -e "$3" ]; then ditto "$3" "$1"; elif [ ! -e "$4" ]; then fail "$3 and $4 are both missing"; fi
}

if [ "${1:-}" = "--restore" ]; then
  B=${2:-}
  [ -n "$B" ] || fail "usage: release-candidate.sh --restore <backup folder>"
  { [ -e "$B/app.linen.reader" ] || [ -e "$B/no-library" ]; } &&
    { [ -e "$B/Linen.app" ] || [ -e "$B/no-app" ]; } ||
    fail "$B is not a backup made by this script"
  running && fail "quit Linen first"
  aside="$B.replaced-$STAMP"
  mkdir "$aside"
  put_back "$DATA" "$aside/app.linen.reader" "$B/app.linen.reader" "$B/no-library"
  put_back "$APP" "$aside/Linen.app" "$B/Linen.app" "$B/no-app"
  echo "Restored from $B."
  echo "The candidate's app and library were moved to $aside; delete it when you no longer need it."
  exit 0
fi

TAG=${1:-}
case $TAG in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *) fail "usage: release-candidate.sh vX.Y.Z | --restore <backup folder>" ;;
esac
VERSION=${TAG#v}
case $(uname -m) in
  arm64) ARCH=aarch64 ;;
  x86_64) ARCH=x64 ;;
  *) fail "no DMG for $(uname -m)" ;;
esac
running && fail "quit Linen first: the library is copied and the app replaced"

# A draft can't be looked up by its tag, so it is found in the list.
REL=$(gh api "repos/$REPO/releases" --paginate --jq ".[] | select(.tag_name == \"$TAG\") | .id" | head -1)
[ -n "$REL" ] || fail "no release or draft for $TAG"
WORK=$(mktemp -d)
fetch() {
  id=$(gh api "repos/$REPO/releases/$REL/assets" --jq ".[] | select(.name == \"$1\") | .id")
  [ -n "$id" ] || fail "$TAG has no $1 yet (has the release workflow finished?)"
  echo "Downloading ${1}…"
  gh api -H 'Accept: application/octet-stream' "repos/$REPO/releases/assets/$id" > "$WORK/$1"
}
DMG="Linen_${VERSION}_${ARCH}.dmg"
ARCHIVE="Linen_${ARCH}.app.tar.gz"
fetch "$DMG"
fetch "$ARCHIVE"
fetch "$ARCHIVE.sig"

# A GitHub asset proves nothing about its bytes, and ad-hoc signatures prove nothing
# about their author: the updater archive must carry the update key's signature for
# this version, and the DMG's app must be that archive's app, byte for byte.
cargo run --quiet --manifest-path "$ROOT/src-tauri/Cargo.toml" --example verify_update -- \
  "$WORK/$ARCHIVE" "$WORK/$ARCHIVE.sig" "$VERSION" ||
  fail "the updater archive is not signed with the update key for $VERSION"
mkdir "$WORK/signed"
tar -xzf "$WORK/$ARCHIVE" -C "$WORK/signed"
MNT="$WORK/mnt"
mkdir "$MNT"
hdiutil attach -nobrowse -readonly -quiet -mountpoint "$MNT" "$WORK/$DMG"
trap 'hdiutil detach -quiet "$MNT" 2>/dev/null || true' EXIT
CAND="$MNT/Linen.app"
diff -rq "$CAND" "$WORK/signed/Linen.app" > /dev/null ||
  fail "the DMG's app differs from the signed updater archive's"
got=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$CAND/Contents/Info.plist")
[ "$got" = "$VERSION" ] || fail "the DMG holds Linen $got, not $VERSION"
codesign --verify --deep --strict "$CAND" || fail "the candidate's code signature does not verify"

# Again: Linen may have been opened while the candidate was downloaded and checked.
running && fail "Linen was opened meanwhile; quit it and run this again"
BACKUP="$(dirname "$DATA")/app.linen.reader.before-$TAG-$STAMP"
mkdir "$BACKUP"
# What was absent is recorded too, so --restore leaves it absent again.
if [ -d "$DATA" ]; then ditto "$DATA" "$BACKUP/app.linen.reader"; else touch "$BACKUP/no-library"; fi
if [ -d "$APP" ]; then mv "$APP" "$BACKUP/Linen.app"; else touch "$BACKUP/no-app"; fi
ditto "$CAND" "$APP"

echo "Linen $VERSION ($ARCH) is installed at $APP, checked against the update key."
echo "Backup of the library and the previous app: $BACKUP"
echo "To undo: scripts/release-candidate.sh --restore \"$BACKUP\""
if [ "${LINEN_RC_NO_OPEN:-}" != 1 ]; then open "$APP"; fi
