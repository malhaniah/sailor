#!/usr/bin/env bash
# Build the libmpv that Zeiler's macOS app bundles, from pinned sources.
#
#   scripts/build-libmpv-macos.sh <out-dir>
#
# Builds for the machine's own architecture (arm64 or x86_64; CI runs it once on
# each), for macOS 12.0 and later (Zeiler's minimumSystemVersion), and writes to
# <out-dir>:
#
#   libmpv-macos-<MPV_VERSION>-<arch>.tar.gz   the bundle scripts/fetch-libmpv.mjs
#                                               pins: lib/*.dylib (install names
#                                               @rpath/<file>, ad-hoc signed),
#                                               licenses/, BUILD-INPUTS.md
#   sources/                                    every source archive built from,
#                                               unchanged, plus this script: the
#                                               complete corresponding source
#   SHA256SUMS-<arch>, BUILD-INPUTS-<arch>.md   the archive's and sources' hashes, and
#                                               the build's record
#
# What is in it, and why:
# - mpv in its LGPL configuration (-Dgpl=false), with libmpv only (no mpv program).
# - FFmpeg without --enable-gpl and without --enable-version3: LGPL-2.1-or-later.
#   HTTPS through macOS's own Secure Transport, video decoding through VideoToolbox
#   (and dav1d for AV1 in software), audio through AudioToolbox.
# - libass with FreeType, HarfBuzz and FriBidi (subtitles; fonts through Core Text),
#   and libplacebo (mpv's renderer) with OpenGL only.
# - mpv's macOS glue (Cocoa, some of it Swift: the Swift runtime is part of macOS
#   12) and its VideoToolbox-to-OpenGL interop, so decoded frames reach the render
#   API without a copy through memory.
# - zlib, bzip2 and iconv come from macOS itself.
# Every archive is pinned by SHA-256 (the same pins as Homebrew's formulae of these
# versions). Nothing is taken from Homebrew's libraries: pkg-config sees only this
# build's prefix. The build tools (meson, ninja, pkg-config, nasm) come from PATH.
#
# To update: bump a version, its URL and its SHA-256 below (and MPV_VERSION), run the
# libmpv-macos workflow, mirror its output as a new pre-release, and update the pins in
# scripts/fetch-libmpv.mjs, docs/libmpv/ and THIRD-PARTY-NOTICES.md; then vendor it
# (`node scripts/fetch-libmpv.mjs`) and rewrite the shipped notices with
# `node scripts/libmpv-macos-notices.mjs` (its component list must match).
set -euo pipefail

MPV_VERSION=0.41.0
DEPLOYMENT_TARGET=12.0

# name|version|url|sha256|licence
SOURCES=(
  "mpv|0.41.0|https://github.com/mpv-player/mpv/archive/refs/tags/v0.41.0.tar.gz|ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209|LGPL-2.1-or-later (-Dgpl=false)"
  "ffmpeg|9.0.2|https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz|8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e|LGPL-2.1-or-later"
  "libplacebo|7.360.1|https://code.videolan.org/videolan/libplacebo/-/archive/v7.360.1/libplacebo-v7.360.1.tar.bz2|937aa5eeea596798b3274d362de2e3bd32bc537a66d149dd85043349c74dffb6|LGPL-2.1-or-later"
  "libass|0.17.5|https://github.com/libass/libass/releases/download/0.17.5/libass-0.17.5.tar.xz|2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc|ISC"
  "freetype|2.15.0|https://downloads.sourceforge.net/project/freetype/freetype2/2.15.0/freetype-2.15.0.tar.xz|93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5|FTL"
  "harfbuzz|14.6.0|https://github.com/harfbuzz/harfbuzz/releases/download/14.6.0/harfbuzz-14.6.0.tar.xz|d07a007327277708a2a73ae437887cdbaf282937f6d03ca5467723e9099af586|MIT-Modern-Variant"
  "fribidi|1.0.17|https://github.com/fribidi/fribidi/releases/download/v1.0.17/fribidi-1.0.17.tar.xz|6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2|LGPL-2.1-or-later"
  "dav1d|1.5.4|https://code.videolan.org/videolan/dav1d/-/archive/1.5.4/dav1d-1.5.4.tar.bz2|2abfb0c89212e6e4733a54e0ae509ec00a5b845a6360946f918806e14aedb011|BSD-2-Clause"
  # build-time only, for libplacebo: fast_float is a header library compiled into it,
  # glad generates its OpenGL loader (with jinja2 and markupsafe), and its Vulkan stubs
  # include the Vulkan headers (no Vulkan code is built)
  "fast_float|8.3.2|https://github.com/fastfloat/fast_float/archive/refs/tags/v8.3.2.tar.gz|71bb9e865b888df01351558db5a39586d13ea97ada162156b3a07784d8c1b26b|MIT OR Apache-2.0 OR BSL-1.0"
  "vulkan-headers|1.4.363.0|https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-1.4.363.0.tar.gz|4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e|Apache-2.0 OR MIT (headers)"
  "glad2|2.0.8|https://files.pythonhosted.org/packages/6e/5a/d62b24fe1c7c2f34e15c2aa4418a5327a8550fdc272999a59e0dddebc3ee/glad2-2.0.8.tar.gz|b84079b9fa404f37171b961bdd1d8da21370e6c818defb8481c5b3fe3d6436da|MIT (the loader it generates: (WTFPL OR CC0-1.0) AND Apache-2.0)"
  "jinja2|3.1.6|https://files.pythonhosted.org/packages/df/bf/f7da0350254c0ed7c72f3e33cef02e048281fec7ecec5f032d4aac52226b/jinja2-3.1.6.tar.gz|0137fb05990d35f1275a587e9aee6d56da821fc83491a0fb838183be43f66d6d|BSD-3-Clause (build tool)"
  "markupsafe|3.0.3|https://files.pythonhosted.org/packages/7e/99/7690b6d4034fffd95959cbe0c02de8deb3098cc577c67bb6a24fe5d7caa7/markupsafe-3.0.3.tar.gz|722695808f4b6457b320fdc131280796bdceb04ab50fe1795cd540799ebe1698|BSD-3-Clause (build tool)"
)

OUT=${1:?usage: scripts/build-libmpv-macos.sh <out-dir>}
[[ "$(uname -s)" == Darwin ]] || { echo "macOS only" >&2; exit 1; }
ARCH=$(uname -m)
case "$ARCH" in arm64 | x86_64) ;; *) echo "unsupported arch $ARCH" >&2; exit 1 ;; esac
for tool in meson ninja pkg-config nasm curl shasum install_name_tool codesign otool; do
  command -v "$tool" >/dev/null || { echo "missing build tool: $tool" >&2; exit 1; }
done

mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
# canonical (no // from a TMPDIR ending in /, no /var vs /private/var), as the install
# names the build writes must match it when they are rewritten below
WORK=$(cd "$(mktemp -d "${TMPDIR:-/tmp}/zeiler-libmpv-macos.XXXXXX")" && pwd -P)
trap 'rm -rf "$WORK"' EXIT
PREFIX="$WORK/prefix"
SRC="$WORK/src"
SOURCES_OUT="$OUT/sources"
mkdir -p "$PREFIX" "$SRC" "$SOURCES_OUT"
JOBS=$(sysctl -n hw.ncpu)
log() { echo "[build-libmpv-macos] $*"; }

field() { local IFS='|'; read -r -a f <<<"$1"; echo "${f[$2]}"; }
entry() {
  for s in "${SOURCES[@]}"; do [[ "$(field "$s" 0)" == "$1" ]] && { echo "$s"; return; }; done
  echo "no source $1" >&2; exit 1
}

# ---- fetch and verify ---------------------------------------------------------------
for s in "${SOURCES[@]}"; do
  url=$(field "$s" 2); sha=$(field "$s" 3); file="$SOURCES_OUT/$(basename "$url")"
  if [[ ! -f "$file" ]] || ! echo "$sha  $file" | shasum -a 256 -c --status; then
    log "fetching $url"
    curl -fsSL --retry 3 -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  echo "$sha  $file" | shasum -a 256 -c --status || {
    echo "checksum mismatch: $file (expected $sha)" >&2; exit 1; }
done
cp "$0" "$SOURCES_OUT/build-libmpv-macos.sh"

# unpack <name> [<dest>]: the archive's single top directory becomes <dest>
unpack() {
  local s file dest
  s=$(entry "$1"); file="$SOURCES_OUT/$(basename "$(field "$s" 2)")"
  dest=${2:-$SRC/$1}
  mkdir -p "$dest"
  tar -xf "$file" -C "$dest" --strip-components 1
  echo "$dest"
}

# ---- environment: this prefix only, macOS 12.0 -----------------------------------------
export MACOSX_DEPLOYMENT_TARGET=$DEPLOYMENT_TARGET
export CFLAGS="-O2 -arch $ARCH -mmacosx-version-min=$DEPLOYMENT_TARGET"
export CXXFLAGS="$CFLAGS"
export OBJCFLAGS="$CFLAGS"
export LDFLAGS="-arch $ARCH -mmacosx-version-min=$DEPLOYMENT_TARGET -Wl,-headerpad_max_install_names"
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH
MESON=(--prefix "$PREFIX" --libdir lib --buildtype release -Ddefault_library=shared
  --wrap-mode nodownload)

# Every object must target $DEPLOYMENT_TARGET: an object built for a newer macOS (the
# linker warns, and stamps the library with the older one anyway) could use symbols
# that older systems lack, and then the app would not start there.
newer_objects() { # <build log>
  if grep -q "built for newer" "$1"; then
    grep "built for newer" "$1" | head -5 >&2
    echo "objects built for a newer macOS than $DEPLOYMENT_TARGET" >&2
    exit 1
  fi
}

meson_build() { # <src> <meson options...>
  local src=$1; shift
  meson setup "$src/build" "$src" "${MESON[@]}" "$@"
  ninja -C "$src/build" -j "$JOBS" 2>&1 | tee "$src/build/ninja.log"
  newer_objects "$src/build/ninja.log"
  ninja -C "$src/build" install
}

log "building for $ARCH, macOS $DEPLOYMENT_TARGET+"

src=$(unpack fribidi)
meson_build "$src" -Ddocs=false -Dbin=false -Dtests=false

src=$(unpack freetype)
meson_build "$src" -Dbrotli=disabled -Dbzip2=disabled -Dharfbuzz=disabled -Dpng=disabled \
  -Dzlib=system -Dtests=disabled

src=$(unpack harfbuzz)
meson_build "$src" -Dfreetype=enabled -Dcoretext=enabled -Dglib=disabled -Dgobject=disabled \
  -Dcairo=disabled -Dchafa=disabled -Dicu=disabled -Dpng=disabled -Dzlib=disabled \
  -Dintrospection=disabled -Dtests=disabled -Ddocs=disabled -Dutilities=disabled \
  -Dgpu=disabled -Dgpu_demo=disabled -Dsubset=disabled -Draster=disabled -Dvector=disabled

src=$(unpack libass)
meson_build "$src" -Dfontconfig=disabled -Dcoretext=enabled -Dlibunibreak=disabled \
  -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled -Dcheckasm=disabled

src=$(unpack dav1d)
meson_build "$src" -Denable_tools=false -Denable_examples=false -Denable_tests=false \
  -Denable_docs=false

src=$(unpack ffmpeg)
FFMPEG_CONFIGURE=(--prefix="$PREFIX" --enable-shared --disable-static --enable-pic
  --disable-programs --disable-doc --disable-debug --disable-avdevice
  --disable-autodetect --enable-videotoolbox --enable-audiotoolbox --enable-securetransport
  --enable-zlib --enable-bzlib --enable-iconv --enable-libdav1d
  --arch="$ARCH" --cc=clang --extra-cflags="$CFLAGS" --extra-ldflags="$LDFLAGS"
  --extra-libs=-liconv)
(cd "$src" && ./configure "${FFMPEG_CONFIGURE[@]}" && make -j "$JOBS" 2>&1 | tee make.log &&
  make install)
newer_objects "$src/make.log"

src=$(unpack libplacebo)
unpack fast_float "$src/3rdparty/fast_float" >/dev/null
unpack vulkan-headers "$src/3rdparty/Vulkan-Headers" >/dev/null
unpack glad2 "$src/3rdparty/glad" >/dev/null
unpack jinja2 "$src/3rdparty/jinja" >/dev/null
unpack markupsafe "$src/3rdparty/markupsafe" >/dev/null
meson_build "$src" -Dvulkan=disabled -Dopengl=enabled -Dd3d11=disabled -Dglslang=disabled \
  -Dshaderc=disabled -Dlcms=disabled -Dlibdovi=disabled -Dunwind=disabled -Dxxhash=disabled \
  -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false

src=$(unpack mpv)
MPV_OPTIONS=(-Dlibmpv=true -Dcplayer=false -Dgpl=false -Dbuild-date=false -Dtests=false
  -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled
  -Ddvdnav=disabled -Dcdda=disabled -Duchardet=disabled -Dlcms2=disabled -Drubberband=disabled
  -Dvapoursynth=disabled -Dzimg=disabled -Djpeg=disabled -Dsixel=disabled -Dlibavdevice=disabled
  -Dsdl2-audio=disabled -Dsdl2-video=disabled -Dsdl2-gamepad=disabled -Dopenal=disabled
  -Djack=disabled -Dvulkan=disabled -Dshaderc=disabled -Dspirv-cross=disabled -Dx11=disabled
  -Dmacos-cocoa-cb=disabled -Dmacos-media-player=disabled -Dmacos-touchbar=disabled
  -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled -Dgl=enabled -Dplain-gl=enabled
  -Dgl-cocoa=enabled -Dvideotoolbox-gl=enabled
  # mpv runs swiftc without a target, which then builds for the build machine's macOS
  "-Dswift-flags=-target $ARCH-apple-macos$DEPLOYMENT_TARGET"
  -Dhtml-build=disabled -Dmanpage-build=disabled -Dpdf-build=disabled)
meson_build "$src" "${MPV_OPTIONS[@]}"

# ---- collect: libmpv and everything it loads from this prefix --------------------------
STAGE="$WORK/stage"
mkdir -p "$STAGE/lib" "$STAGE/licenses"
deps_of() { otool -L "$1" | tail -n +2 | awk '{print $1}'; }
queue=("$PREFIX/lib/libmpv.2.dylib")
while ((${#queue[@]})); do
  lib=${queue[0]}; queue=("${queue[@]:1}")
  name=$(basename "$lib")
  [[ -f "$STAGE/lib/$name" ]] && continue
  cp -L "$lib" "$STAGE/lib/$name"
  chmod u+w "$STAGE/lib/$name"
  while read -r dep; do
    case "$dep" in
      "$PREFIX"/*) queue+=("$dep") ;;
      @rpath/*) queue+=("$PREFIX/lib/${dep#@rpath/}") ;;
    esac
  done < <(deps_of "$lib")
done
# install names: @rpath/<file>, found through the app's @executable_path/../Frameworks
for lib in "$STAGE"/lib/*.dylib; do
  name=$(basename "$lib")
  install_name_tool -id "@rpath/$name" "$lib"
  while read -r dep; do
    case "$dep" in
      "$PREFIX"/*) install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$lib" ;;
    esac
  done < <(deps_of "$lib")
  # rpaths: meson's point into this build's prefix, and the Swift link adds the build
  # machine's toolchain; only macOS's own Swift runtime (/usr/lib/swift) stays
  while read -r rpath; do
    install_name_tool -delete_rpath "$rpath" "$lib"
  done < <(otool -l "$lib" | awk '/LC_RPATH/{getline; getline; print $2}' | grep -vx /usr/lib/swift || true)
done
# nothing may point outside the bundle or the system
bad=$(for lib in "$STAGE"/lib/*.dylib; do deps_of "$lib"; done |
  grep -vE '^(@rpath/|/usr/lib/|/System/Library/)' || true)
[[ -z "$bad" ]] || { echo "unbundled dependencies:"$'\n'"$bad" >&2; exit 1; }
for lib in "$STAGE"/lib/*.dylib; do
  minos=$(otool -l "$lib" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')
  [[ "$minos" == "$DEPLOYMENT_TARGET" ]] || {
    echo "$(basename "$lib") targets macOS $minos, not $DEPLOYMENT_TARGET" >&2; exit 1; }
  lipo -archs "$lib" | grep -qx "$ARCH" || { echo "$(basename "$lib") is not $ARCH" >&2; exit 1; }
  codesign --force --sign - "$lib"
done
# for -lmpv at link time (libmpv2-sys links `mpv`)
ln -s libmpv.2.dylib "$STAGE/lib/libmpv.dylib"

# ---- licences, as each project ships them ----------------------------------------------
copy_licences() { # <name> <files...>
  mkdir -p "$STAGE/licenses/$1"
  local dir=$SRC/$1; shift
  for f in "$@"; do cp "$dir/$f" "$STAGE/licenses/$(basename "$dir")/"; done
}
copy_licences mpv LICENSE.LGPL Copyright
copy_licences ffmpeg LICENSE.md COPYING.LGPLv2.1
copy_licences libplacebo LICENSE
copy_licences libass COPYING
copy_licences freetype LICENSE.TXT docs/FTL.TXT
copy_licences harfbuzz COPYING
copy_licences fribidi COPYING AUTHORS
copy_licences dav1d COPYING
mkdir -p "$STAGE/licenses/fast_float" "$STAGE/licenses/glad" "$STAGE/licenses/vulkan-headers"
cp "$SRC/libplacebo/3rdparty/fast_float/LICENSE-MIT" "$STAGE/licenses/fast_float/"
# the loader glad generates: (WTFPL OR CC0-1.0) AND Apache-2.0 (from Khronos' gl.xml)
cp "$SRC/libplacebo/3rdparty/glad/LICENSE" "$STAGE/licenses/glad/"
cp "$SRC/libplacebo/3rdparty/Vulkan-Headers/LICENSES/Apache-2.0.txt" "$STAGE/licenses/glad/"
cp "$SRC/libplacebo/3rdparty/Vulkan-Headers/LICENSE.md" \
  "$SRC/libplacebo/3rdparty/Vulkan-Headers/LICENSES/MIT.txt" "$STAGE/licenses/vulkan-headers/"

# ---- the record ---------------------------------------------------------------------------
{
  echo "# libmpv for macOS ($ARCH): build inputs"
  echo
  echo "Built by \`scripts/build-libmpv-macos.sh\` (a copy is in sources/)."
  [[ -n "${GITHUB_RUN_ID:-}" ]] &&
    echo "Workflow run: ${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-}/actions/runs/$GITHUB_RUN_ID (commit ${GITHUB_SHA:-?}, runner ${RUNNER_NAME:-?})."
  echo
  echo "- Architecture: $ARCH; deployment target: macOS $DEPLOYMENT_TARGET"
  echo "- Host: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion)), SDK $(xcrun --show-sdk-version)"
  echo "- Compiler: $(clang --version | head -1)"
  echo "- meson $(meson --version), ninja $(ninja --version), pkg-config $(pkg-config --version), nasm $(nasm -v | awk '{print $3}')"
  echo
  echo "| Component | Version | Licence | Source archive | SHA-256 |"
  echo "|---|---|---|---|---|"
  for s in "${SOURCES[@]}"; do
    echo "| $(field "$s" 0) | $(field "$s" 1) | $(field "$s" 4) | $(field "$s" 2) | \`$(field "$s" 3)\` |"
  done
  echo
  echo "FFmpeg configure: \`${FFMPEG_CONFIGURE[*]}\`"
  echo
  echo "mpv meson options: \`${MPV_OPTIONS[*]}\`"
  echo
  echo "## Libraries"
  echo
  echo "| File | SHA-256 |"
  echo "|---|---|"
  for lib in "$STAGE"/lib/*.dylib; do
    [[ -L "$lib" ]] && continue
    echo "| $(basename "$lib") | \`$(shasum -a 256 "$lib" | awk '{print $1}')\` |"
  done
} >"$STAGE/BUILD-INPUTS.md"

ARCHIVE="libmpv-macos-$MPV_VERSION-$ARCH.tar.gz"
# sorted, with fixed owners and times, so the archive depends only on its files
(cd "$STAGE" && find . -exec touch -h -t 202601010000 {} + &&
  tar --uid 0 --gid 0 --uname root --gname wheel -czf "$OUT/$ARCHIVE" \
    $(find lib licenses BUILD-INPUTS.md -type f -o -type l | LC_ALL=C sort))
cp "$STAGE/BUILD-INPUTS.md" "$OUT/BUILD-INPUTS-$ARCH.md"
(cd "$OUT" && shasum -a 256 "$ARCHIVE" sources/* >"SHA256SUMS-$ARCH")
log "done: $OUT/$ARCHIVE"
cat "$OUT/SHA256SUMS-$ARCH"
