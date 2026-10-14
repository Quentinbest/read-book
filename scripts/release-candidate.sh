#!/bin/sh
# Release acceptance (docs/release-acceptance.md): put a release candidate on this
# Mac for the owner's hand test. Downloads the draft release's DMG for this Mac,
# checks its version and signature, backs up the library folder and the installed
# app, installs the candidate and opens it. --restore puts a backup back.
# Nothing is deleted: whatever is replaced is moved aside.
#
#   scripts/release-candidate.sh v0.5.2              install the candidate
#   scripts/release-candidate.sh --restore <backup>  put that backup back
#
# LINEN_RC_APP and LINEN_RC_DATA point it at another app and library folder, and
# LINEN_RC_NO_OPEN=1 leaves the app closed (to try the script without touching the
# installed Linen).
set -eu

REPO=Quentinbest/read-book
APP=${LINEN_RC_APP:-/Applications/Linen.app}
DATA=${LINEN_RC_DATA:-"$HOME/Library/Application Support/app.linen.reader"}
STAMP=$(date +%Y%m%d-%H%M%S)

fail() { echo "release-candidate: $*" >&2; exit 1; }
# The library must not change while it is copied, and a running app can't be replaced.
running() { pgrep -qf "$APP/Contents/MacOS/"; }

if [ "${1:-}" = "--restore" ]; then
  B=${2:-}
  [ -n "$B" ] || fail "usage: release-candidate.sh --restore <backup folder>"
  [ -d "$B/app.linen.reader" ] || [ -d "$B/Linen.app" ] || fail "$B is not a backup made by this script"
  running && fail "quit Linen first"
  aside="$B.replaced-$STAMP"
  mkdir "$aside"
  if [ -d "$B/app.linen.reader" ]; then
    if [ -e "$DATA" ]; then mv "$DATA" "$aside/app.linen.reader"; fi
    ditto "$B/app.linen.reader" "$DATA"
  fi
  if [ -d "$B/Linen.app" ]; then
    if [ -e "$APP" ]; then mv "$APP" "$aside/Linen.app"; fi
    ditto "$B/Linen.app" "$APP"
  fi
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
NAME="Linen_${VERSION}_${ARCH}.dmg"
ASSET=$(gh api "repos/$REPO/releases/$REL/assets" --jq ".[] | select(.name == \"$NAME\") | .id")
[ -n "$ASSET" ] || fail "$TAG has no $NAME yet (has the release workflow finished?)"

WORK=$(mktemp -d)
echo "Downloading ${NAME}…"
gh api -H 'Accept: application/octet-stream' "repos/$REPO/releases/assets/$ASSET" > "$WORK/$NAME"
MNT="$WORK/mnt"
mkdir "$MNT"
hdiutil attach -nobrowse -readonly -quiet -mountpoint "$MNT" "$WORK/$NAME"
trap 'hdiutil detach -quiet "$MNT" 2>/dev/null || true' EXIT
CAND="$MNT/Linen.app"
got=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$CAND/Contents/Info.plist")
[ "$got" = "$VERSION" ] || fail "the DMG holds Linen $got, not $VERSION"
codesign --verify --deep --strict "$CAND" || fail "the candidate's signature does not verify"

BACKUP="$(dirname "$DATA")/app.linen.reader.before-$TAG-$STAMP"
mkdir "$BACKUP"
if [ -d "$DATA" ]; then ditto "$DATA" "$BACKUP/app.linen.reader"; fi
if [ -d "$APP" ]; then mv "$APP" "$BACKUP/Linen.app"; fi
ditto "$CAND" "$APP"

echo "Linen $VERSION ($ARCH) is installed at $APP."
echo "Backup of the library and the previous app: $BACKUP"
echo "To undo: scripts/release-candidate.sh --restore \"$BACKUP\""
if [ "${LINEN_RC_NO_OPEN:-}" != 1 ]; then open "$APP"; fi
