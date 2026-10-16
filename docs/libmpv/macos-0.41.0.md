# libmpv for macOS 0.41.0: build inputs

Zeiler's macOS app ships libmpv and the libraries it loads in `Zeiler.app/Contents/Frameworks`.
Zeiler builds them itself from unchanged upstream source releases with
`scripts/build-libmpv-macos.sh`, in the `libmpv-macos` workflow, once per architecture, and
keeps the archives, every source archive, the build script and this record on the
pre-release [`libmpv-macos-0.41.0`](https://github.com/malhaniah/zeiler/releases/tag/libmpv-macos-0.41.0).
That release is the source offer's "complete corresponding source" for this build.

## The archives

| Architecture | Archive | SHA-256 |
|---|---|---|
| arm64 | `libmpv-macos-0.41.0-arm64.tar.gz` | `6150075c216dfe8c868ff0a4e60cd7d6f91e6f9df369019a83cf058889449d75` |
| x86_64 | `libmpv-macos-0.41.0-x86_64.tar.gz` | `938835375e1f39792a0d46e5b0c9fa62b6fbb690521486366c39cbac54275dc0` |

Both come from workflow run [38973750902](https://github.com/malhaniah/zeiler/actions/runs/38973750902)
(pull request #21). `scripts/fetch-libmpv.mjs` downloads the one for the build's
architecture from the release and checks that hash. Each holds `lib/` (the libraries,
install names `@rpath/<file>`, ad-hoc signed, macOS 12.0 deployment target), `licenses/`
(each component's licence files) and its `BUILD-INPUTS.md` (below).

## Configuration

mpv in its LGPL configuration (`-Dgpl=false`, libmpv only, no Lua or JavaScript) with its
Cocoa/Swift glue and VideoToolbox-to-OpenGL interop; FFmpeg without `--enable-gpl` and
without `--enable-version3`, with `--disable-autodetect` and only VideoToolbox,
AudioToolbox, Secure Transport, zlib, bzlib, iconv (all from macOS) and dav1d enabled;
libplacebo with OpenGL only; libass with FreeType, HarfBuzz and FriBidi (fonts through
Core Text). The exact options are in each architecture's record below and in the script.

## arm64

Built by `scripts/build-libmpv-macos.sh` (a copy is in sources/).
Workflow run: https://github.com/malhaniah/zeiler/actions/runs/38973750902 (commit 4623475fa6a7dfd51d7e2ef737673b08e8e9d04e, runner GitHub Actions 1000000338).

- Architecture: arm64; deployment target: macOS 12.0
- Host: macOS 15.7.9 (24G830), SDK 15.5
- Compiler: Apple clang version 17.0.0 (clang-1700.0.13.5)
- meson 1.12.0, ninja 1.13.2, pkg-config 3.0.7, nasm 3.02

| Component | Version | Licence | Source archive | SHA-256 |
|---|---|---|---|---|
| mpv | 0.41.0 | LGPL-2.1-or-later (-Dgpl=false) | https://github.com/mpv-player/mpv/archive/refs/tags/v0.41.0.tar.gz | `ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209` |
| ffmpeg | 9.0.2 | LGPL-2.1-or-later | https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz | `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e` |
| libplacebo | 7.360.1 | LGPL-2.1-or-later | https://code.videolan.org/videolan/libplacebo/-/archive/v7.360.1/libplacebo-v7.360.1.tar.bz2 | `937aa5eeea596798b3274d362de2e3bd32bc537a66d149dd85043349c74dffb6` |
| libass | 0.17.5 | ISC | https://github.com/libass/libass/releases/download/0.17.5/libass-0.17.5.tar.xz | `2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc` |
| freetype | 2.15.0 | FTL | https://downloads.sourceforge.net/project/freetype/freetype2/2.15.0/freetype-2.15.0.tar.xz | `93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5` |
| harfbuzz | 14.6.0 | MIT-Modern-Variant | https://github.com/harfbuzz/harfbuzz/releases/download/14.6.0/harfbuzz-14.6.0.tar.xz | `d07a007327277708a2a73ae437887cdbaf282937f6d03ca5467723e9099af586` |
| fribidi | 1.0.17 | LGPL-2.1-or-later | https://github.com/fribidi/fribidi/releases/download/v1.0.17/fribidi-1.0.17.tar.xz | `6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2` |
| dav1d | 1.5.4 | BSD-2-Clause | https://code.videolan.org/videolan/dav1d/-/archive/1.5.4/dav1d-1.5.4.tar.bz2 | `2abfb0c89212e6e4733a54e0ae509ec00a5b845a6360946f918806e14aedb011` |
| fast_float | 8.3.2 | MIT OR Apache-2.0 OR BSL-1.0 | https://github.com/fastfloat/fast_float/archive/refs/tags/v8.3.2.tar.gz | `71bb9e865b888df01351558db5a39586d13ea97ada162156b3a07784d8c1b26b` |
| vulkan-headers | 1.4.363.0 | Apache-2.0 OR MIT (headers) | https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-1.4.363.0.tar.gz | `4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e` |
| glad2 | 2.0.8 | MIT (the loader it generates: (WTFPL OR CC0-1.0) AND Apache-2.0) | https://files.pythonhosted.org/packages/6e/5a/d62b24fe1c7c2f34e15c2aa4418a5327a8550fdc272999a59e0dddebc3ee/glad2-2.0.8.tar.gz | `b84079b9fa404f37171b961bdd1d8da21370e6c818defb8481c5b3fe3d6436da` |
| jinja2 | 3.1.6 | BSD-3-Clause (build tool) | https://files.pythonhosted.org/packages/df/bf/f7da0350254c0ed7c72f3e33cef02e048281fec7ecec5f032d4aac52226b/jinja2-3.1.6.tar.gz | `0137fb05990d35f1275a587e9aee6d56da821fc83491a0fb838183be43f66d6d` |
| markupsafe | 3.0.3 | BSD-3-Clause (build tool) | https://files.pythonhosted.org/packages/7e/99/7690b6d4034fffd95959cbe0c02de8deb3098cc577c67bb6a24fe5d7caa7/markupsafe-3.0.3.tar.gz | `722695808f4b6457b320fdc131280796bdceb04ab50fe1795cd540799ebe1698` |

FFmpeg configure: `--prefix=/private/var/folders/nj/vtw8zd2j31d1gdrtntc5y4600000gn/T/zeiler-libmpv-macos.kD0ypw/prefix --enable-shared --disable-static --enable-pic --disable-programs --disable-doc --disable-debug --disable-avdevice --disable-autodetect --enable-videotoolbox --enable-audiotoolbox --enable-securetransport --enable-zlib --enable-bzlib --enable-iconv --enable-libdav1d --arch=arm64 --cc=clang --extra-cflags=-O2 -arch arm64 -mmacosx-version-min=12.0 --extra-ldflags=-arch arm64 -mmacosx-version-min=12.0 -Wl,-headerpad_max_install_names --extra-libs=-liconv`

mpv meson options: `-Dlibmpv=true -Dcplayer=false -Dgpl=false -Dbuild-date=false -Dtests=false -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled -Ddvdnav=disabled -Dcdda=disabled -Duchardet=disabled -Dlcms2=disabled -Drubberband=disabled -Dvapoursynth=disabled -Dzimg=disabled -Djpeg=disabled -Dsixel=disabled -Dlibavdevice=disabled -Dsdl2-audio=disabled -Dsdl2-video=disabled -Dsdl2-gamepad=disabled -Dopenal=disabled -Djack=disabled -Dvulkan=disabled -Dshaderc=disabled -Dspirv-cross=disabled -Dx11=disabled -Dmacos-cocoa-cb=disabled -Dmacos-media-player=disabled -Dmacos-touchbar=disabled -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled -Dgl=enabled -Dplain-gl=enabled -Dgl-cocoa=enabled -Dvideotoolbox-gl=enabled -Dswift-flags=-target arm64-apple-macos12.0 -Dhtml-build=disabled -Dmanpage-build=disabled -Dpdf-build=disabled`

### Libraries

| File | SHA-256 |
|---|---|
| libass.9.dylib | `a5a6fea5ae32fb5c699fb5ce60bcaaf3992e7045b72a7d95b820d25d57a784b4` |
| libavcodec.63.dylib | `af6e62b96ddb7ddb4f16df914da8c1bf5963a5baa94f507808111f3cb6324a01` |
| libavfilter.12.dylib | `96214a02045215d9beee4bcc2e9837356a3391bf69b50dcb715efe334f92d1e4` |
| libavformat.63.dylib | `a56cbea347a2a96e5c72ca5da717d85fd810f6b611b6d8210b23bf164f80dc55` |
| libavutil.61.dylib | `cb31afd9c1625ee13a826badf6776b04030257f5a5b16b958aa4b72e2eda8fbe` |
| libdav1d.7.dylib | `937c26e62f9e1fafadd3ff078966cb35fc10e348f3489567b126615df7d397f9` |
| libfreetype.6.dylib | `8402a91acc4fb424ad81a0c1b0d5b0a721404e4121af9afccf09cbfe8a7edb4f` |
| libfribidi.0.dylib | `6b4b3cf09a67c18cb87de6d8463617d90fd17d4ed0ab7a36537c662fe7985703` |
| libharfbuzz.0.dylib | `4a3933fe8ab6a8804a6c6f8aaf9d72c34cf660f99ef2ebca32bfd8ec55f20268` |
| libmpv.2.dylib | `ad9d1cd4d68f01626a38128cc3878351d8a2bde0b35af4da2227665aa4c9e0d1` |
| libplacebo.360.dylib | `092ea697e1418043be34ea950851083130cc5e8d9b3bfce197c56d912c2a686d` |
| libswresample.7.dylib | `02a1cd0a2c532ffe06693793e076da1082d69a88be77e71b1788b03288825e12` |
| libswscale.10.dylib | `e0dd80e83c6e0042c34ea07a085a2e3d072f042280661942042f75d083fc117c` |

## x86_64

Built by `scripts/build-libmpv-macos.sh` (a copy is in sources/).
Workflow run: https://github.com/malhaniah/zeiler/actions/runs/38973750902 (commit 4623475fa6a7dfd51d7e2ef737673b08e8e9d04e, runner GitHub Actions 1000000337).

- Architecture: x86_64; deployment target: macOS 12.0
- Host: macOS 15.7.9 (24G830), SDK 15.5
- Compiler: Apple clang version 17.0.0 (clang-1700.0.13.5)
- meson 1.12.0, ninja 1.13.2, pkg-config 3.0.5, nasm 3.02

| Component | Version | Licence | Source archive | SHA-256 |
|---|---|---|---|---|
| mpv | 0.41.0 | LGPL-2.1-or-later (-Dgpl=false) | https://github.com/mpv-player/mpv/archive/refs/tags/v0.41.0.tar.gz | `ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209` |
| ffmpeg | 9.0.2 | LGPL-2.1-or-later | https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz | `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e` |
| libplacebo | 7.360.1 | LGPL-2.1-or-later | https://code.videolan.org/videolan/libplacebo/-/archive/v7.360.1/libplacebo-v7.360.1.tar.bz2 | `937aa5eeea596798b3274d362de2e3bd32bc537a66d149dd85043349c74dffb6` |
| libass | 0.17.5 | ISC | https://github.com/libass/libass/releases/download/0.17.5/libass-0.17.5.tar.xz | `2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc` |
| freetype | 2.15.0 | FTL | https://downloads.sourceforge.net/project/freetype/freetype2/2.15.0/freetype-2.15.0.tar.xz | `93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5` |
| harfbuzz | 14.6.0 | MIT-Modern-Variant | https://github.com/harfbuzz/harfbuzz/releases/download/14.6.0/harfbuzz-14.6.0.tar.xz | `d07a007327277708a2a73ae437887cdbaf282937f6d03ca5467723e9099af586` |
| fribidi | 1.0.17 | LGPL-2.1-or-later | https://github.com/fribidi/fribidi/releases/download/v1.0.17/fribidi-1.0.17.tar.xz | `6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2` |
| dav1d | 1.5.4 | BSD-2-Clause | https://code.videolan.org/videolan/dav1d/-/archive/1.5.4/dav1d-1.5.4.tar.bz2 | `2abfb0c89212e6e4733a54e0ae509ec00a5b845a6360946f918806e14aedb011` |
| fast_float | 8.3.2 | MIT OR Apache-2.0 OR BSL-1.0 | https://github.com/fastfloat/fast_float/archive/refs/tags/v8.3.2.tar.gz | `71bb9e865b888df01351558db5a39586d13ea97ada162156b3a07784d8c1b26b` |
| vulkan-headers | 1.4.363.0 | Apache-2.0 OR MIT (headers) | https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-1.4.363.0.tar.gz | `4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e` |
| glad2 | 2.0.8 | MIT (the loader it generates: (WTFPL OR CC0-1.0) AND Apache-2.0) | https://files.pythonhosted.org/packages/6e/5a/d62b24fe1c7c2f34e15c2aa4418a5327a8550fdc272999a59e0dddebc3ee/glad2-2.0.8.tar.gz | `b84079b9fa404f37171b961bdd1d8da21370e6c818defb8481c5b3fe3d6436da` |
| jinja2 | 3.1.6 | BSD-3-Clause (build tool) | https://files.pythonhosted.org/packages/df/bf/f7da0350254c0ed7c72f3e33cef02e048281fec7ecec5f032d4aac52226b/jinja2-3.1.6.tar.gz | `0137fb05990d35f1275a587e9aee6d56da821fc83491a0fb838183be43f66d6d` |
| markupsafe | 3.0.3 | BSD-3-Clause (build tool) | https://files.pythonhosted.org/packages/7e/99/7690b6d4034fffd95959cbe0c02de8deb3098cc577c67bb6a24fe5d7caa7/markupsafe-3.0.3.tar.gz | `722695808f4b6457b320fdc131280796bdceb04ab50fe1795cd540799ebe1698` |

FFmpeg configure: `--prefix=/private/var/folders/20/jp1_0n3n7kndh6rnbqb5344m0000gn/T/zeiler-libmpv-macos.h1mHLI/prefix --enable-shared --disable-static --enable-pic --disable-programs --disable-doc --disable-debug --disable-avdevice --disable-autodetect --enable-videotoolbox --enable-audiotoolbox --enable-securetransport --enable-zlib --enable-bzlib --enable-iconv --enable-libdav1d --arch=x86_64 --cc=clang --extra-cflags=-O2 -arch x86_64 -mmacosx-version-min=12.0 --extra-ldflags=-arch x86_64 -mmacosx-version-min=12.0 -Wl,-headerpad_max_install_names --extra-libs=-liconv`

mpv meson options: `-Dlibmpv=true -Dcplayer=false -Dgpl=false -Dbuild-date=false -Dtests=false -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled -Ddvdnav=disabled -Dcdda=disabled -Duchardet=disabled -Dlcms2=disabled -Drubberband=disabled -Dvapoursynth=disabled -Dzimg=disabled -Djpeg=disabled -Dsixel=disabled -Dlibavdevice=disabled -Dsdl2-audio=disabled -Dsdl2-video=disabled -Dsdl2-gamepad=disabled -Dopenal=disabled -Djack=disabled -Dvulkan=disabled -Dshaderc=disabled -Dspirv-cross=disabled -Dx11=disabled -Dmacos-cocoa-cb=disabled -Dmacos-media-player=disabled -Dmacos-touchbar=disabled -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled -Dgl=enabled -Dplain-gl=enabled -Dgl-cocoa=enabled -Dvideotoolbox-gl=enabled -Dswift-flags=-target x86_64-apple-macos12.0 -Dhtml-build=disabled -Dmanpage-build=disabled -Dpdf-build=disabled`

### Libraries

| File | SHA-256 |
|---|---|
| libass.9.dylib | `5113e7d5650878bfd92f26d455c3e864d693c8b96736598097b9c43a8a1391f8` |
| libavcodec.63.dylib | `f3ddfa4fe9b31343d9ac277f83cb364a0f046cf0df3bb87b14ad168ad7a33ca6` |
| libavfilter.12.dylib | `f7686e77f3f73ea62545a3700eda97f0aa71f0206469f28e8bc2d14556902e56` |
| libavformat.63.dylib | `66bdf6f17fa87786b76e6d322dad1334d65158c2ddca3f9787099342c22702d0` |
| libavutil.61.dylib | `1eb56513e0b754026c78ce98c061016ca6a84458e0127af3953e4b0c0c4a0450` |
| libdav1d.7.dylib | `b2eea6cfbf2227e8a4f240b9113e4f507c1f218ae0069521d033053173e01b90` |
| libfreetype.6.dylib | `fa9d817c6b002a56e1e68d6fb5a69a14d903f510e688d88ec00ac1c9a0bb0706` |
| libfribidi.0.dylib | `481374a92dbf0fe6e1102f530e7220972fc5342b0d9545760c6ed3451a6871bd` |
| libharfbuzz.0.dylib | `80d21177d72fb8f9f22bba1b2cdf88bc343dc3e5c02b03647f51dda58ca1eb64` |
| libmpv.2.dylib | `4ec9e698055ab3c4a272d9484d3c94b0806e28f38bd34bdd8c2b0ea7bcdebe6b` |
| libplacebo.360.dylib | `86f38130940c2b69db71f0fcd61c55a50dad3df07a3e53d1dc9e91f05a859421` |
| libswresample.7.dylib | `8c64e8351824ad98cb76a32b4d37b6a30ee6da49aefcefea110ef9d6f2d6c9ff` |
| libswscale.10.dylib | `b9dce410bac6ec5628160d144506eaa138b8b1c8dd8feb5abf992b480c8864fe` |

