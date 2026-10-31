#!/usr/bin/env bash
# Скачивает тулчейн и SDL-webOS для кросс-сборки NovaOS.
#
#   tools/setup-webos-sdk.sh [КАТАЛОГ]     # по умолчанию $HOME/.cache/novaos
#
set -euo pipefail

DEST="${1:-${NOVA_WEBOS_HOME:-$HOME/.cache/novaos}}"
NDK_URL="https://github.com/openlgtv/buildroot-nc4/releases/download/webos-b17b4cc/arm-webos-linux-gnueabi_sdk-buildroot.tar.gz"
SDL_URL="https://github.com/webosbrew/SDL-webOS/releases/download/release-2.30.12-webos.5/SDL2-2.30.12-webos.tar.gz"

mkdir -p "$DEST/toolchain" "$DEST/sdl/sdl2"

if [ ! -x "$DEST/toolchain/arm-webos-linux-gnueabi_sdk-buildroot/bin/arm-webos-linux-gnueabi-gcc" ]; then
    echo "==> NDK webOS"
    curl --fail -L --retry 3 -o "$DEST/toolchain/ndk.tar.gz" "$NDK_URL"
    tar -xzf "$DEST/toolchain/ndk.tar.gz" -C "$DEST/toolchain" || true
else
    echo "==> NDK уже распакован"
fi

if [ ! -f "$DEST/sdl/sdl2/lib/pkgconfig/sdl2.pc" ]; then
    echo "==> SDL-webOS"
    curl --fail -L --retry 3 -o "$DEST/sdl/SDL2-webos.tar.gz" "$SDL_URL"
    tar -xzf "$DEST/sdl/SDL2-webos.tar.gz" -C "$DEST/sdl/sdl2"
else
    echo "==> SDL-webOS уже распакован"
fi

cat <<EOF

Готово. Для сборки выполните:

    NOVA_WEBOS_NDK=$DEST/toolchain/arm-webos-linux-gnueabi_sdk-buildroot \\
    NOVA_WEBOS_SDL=$DEST/sdl/sdl2 \\
    tools/build-webos.sh
EOF
