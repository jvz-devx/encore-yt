#!/usr/bin/env bash
# Builds the mpv the Linux AppImage bundles: mpv, FFmpeg and libplacebo from
# source, LGPL only (no GPL parts), audio only (PulseAudio and ALSA output),
# with GnuTLS for https streams. Distribution mpv packages on older systems
# (Ubuntu 22.04 has 0.34) lack options the app sets (volume-gain needs 0.38).
#
# Usage: build-mpv.sh <prefix>   (installs into <prefix>, binary in bin/mpv)
# Needs (Ubuntu 22.04): build-essential nasm pkg-config python3-pip git curl
#   libass-dev libgnutls28-dev zlib1g-dev libpulse-dev libasound2-dev,
#   and `pip install meson ninja jinja2`.
# The versions below also key the CI cache; change them here only.
set -euo pipefail

FFMPEG_VERSION=7.1.2
LIBPLACEBO_VERSION=v7.351.0
MPV_VERSION=v0.41.0

prefix="$(realpath -m "${1:?usage: build-mpv.sh <prefix>}")"
jobs="${JOBS:-$(nproc)}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/lib/x86_64-linux-gnu/pkgconfig"
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# FFmpeg: shared libraries, LGPL (no --enable-gpl/--enable-nonfree), nothing
# picked up from the build machine but GnuTLS and zlib.
# Each library step is skipped when its result is already in <prefix>.
if [[ ! -e "$prefix/lib/libavcodec.so" ]]; then (
    curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" | tar xJ -C "$work"
    cd "$work/ffmpeg-$FFMPEG_VERSION"
    ./configure --prefix="$prefix" --libdir="$prefix/lib" \
        --enable-shared --disable-static \
        --disable-programs --disable-doc --disable-debug \
        --disable-autodetect --enable-gnutls --enable-zlib \
        --disable-avdevice --disable-indevs --disable-outdevs \
        --disable-encoders --disable-muxers --disable-hwaccels
    make -j"$jobs"
    make install
); fi

# libplacebo: mpv requires it; without Vulkan/OpenGL it is a small library.
if [[ ! -e "$prefix/lib/libplacebo.so" ]]; then (
    git clone -q --depth 1 --recursive --branch "$LIBPLACEBO_VERSION" \
        https://code.videolan.org/videolan/libplacebo.git "$work/libplacebo"
    cd "$work/libplacebo"
    meson setup build --prefix="$prefix" --libdir=lib --buildtype=release \
        -Dvulkan=disabled -Dopengl=disabled -Dd3d11=disabled \
        -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled \
        -Ddovi=disabled -Dlibdovi=disabled -Dunwind=disabled -Dxxhash=disabled \
        -Ddemos=false -Dtests=false
    meson install -C build
); fi

# mpv: the player only, every optional feature off except audio output.
(
    curl -fsSL "https://github.com/mpv-player/mpv/archive/refs/tags/$MPV_VERSION.tar.gz" | tar xz -C "$work"
    cd "$work/mpv-${MPV_VERSION#v}"
    meson setup build --prefix="$prefix" --libdir=lib --buildtype=release \
        -Dauto_features=disabled -Dgpl=false -Dcplayer=true -Dlibmpv=false \
        -Dgl=disabled -Dvulkan=disabled \
        -Dpulse=enabled -Dalsa=enabled -Diconv=enabled -Dzlib=enabled
    meson install -C build
)

"$prefix/bin/mpv" --version
