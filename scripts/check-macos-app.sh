#!/usr/bin/env bash
# Check a macOS Zeiler.app built with built-in playback (`--features embedded`):
#
#   scripts/check-macos-app.sh path/to/Zeiler.app
#
# - the app binary loads libmpv as @rpath/libmpv.2.dylib and finds it through
#   @executable_path/../Frameworks, with no other (absolute) rpath;
# - every library the binary and Contents/Frameworks load is either in
#   Contents/Frameworks or part of macOS: nothing from Homebrew (/opt/homebrew,
#   /usr/local) or a build directory, and no library searches anywhere else (rpaths);
# - no library needs a newer macOS than the app's LSMinimumSystemVersion;
# - the bundle's signature (ad-hoc, for unsigned builds) verifies, deep and strict.
set -euo pipefail

APP=${1:?usage: scripts/check-macos-app.sh path/to/Zeiler.app}
CONTENTS="$APP/Contents"
BIN="$CONTENTS/MacOS/zeiler"
FRAMEWORKS="$CONTENTS/Frameworks"
fail() { echo "check-macos-app: $*" >&2; exit 1; }
deps_of() { otool -L "$1" | tail -n +2 | awk '{print $1}'; }

[[ -x "$BIN" ]] || fail "no binary at $BIN"
[[ -f "$FRAMEWORKS/libmpv.2.dylib" ]] || fail "libmpv.2.dylib is not in Contents/Frameworks"
deps_of "$BIN" | grep -qx "@rpath/libmpv.2.dylib" || fail "the binary does not load @rpath/libmpv.2.dylib"

rpaths=$(otool -l "$BIN" | awk '/LC_RPATH/{getline; getline; print $2}')
[[ "$rpaths" == "@executable_path/../Frameworks" ]] ||
  fail "the binary's rpaths should be @executable_path/../Frameworks only, not:"$'\n'"$rpaths"

minimum=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" "$CONTENTS/Info.plist")
version_le() { [[ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" == "$1" ]]; }

for file in "$BIN" "$FRAMEWORKS"/*.dylib; do
  while read -r dep; do
    case "$dep" in
      /usr/lib/* | /System/Library/*) ;;
      @rpath/*) [[ -f "$FRAMEWORKS/${dep#@rpath/}" ]] ||
        fail "$(basename "$file") loads $dep, which is not in Contents/Frameworks" ;;
      *) fail "$(basename "$file") loads $dep, outside the app and the system" ;;
    esac
  done < <(deps_of "$file")
  if [[ "$file" != "$BIN" ]]; then
    while read -r rpath; do
      [[ "$rpath" == /usr/lib/swift ]] ||
        fail "$(basename "$file") has the rpath $rpath (only /usr/lib/swift may stay)"
    done < <(otool -l "$file" | awk '/LC_RPATH/{getline; getline; print $2}')
  fi
  minos=$(otool -l "$file" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')
  [[ -n "$minos" ]] && { version_le "$minos" "$minimum" ||
    fail "$(basename "$file") needs macOS $minos, the app promises $minimum"; }
done

codesign --verify --deep --strict --verbose=2 "$APP"
echo "check-macos-app: $APP is self-contained (macOS $minimum+) and its signature verifies"
