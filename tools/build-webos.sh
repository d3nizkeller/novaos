#!/usr/bin/env bash
# Кросс-сборка NovaOS под LG webOS TV (armv7, soft-float) и упаковка в IPK.
#
#   tools/build-webos.sh            # сборка + IPK + repo.json в dist/
#   NOVA_WEBOS_NDK=/путь tools/build-webos.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NDK="${NOVA_WEBOS_NDK:-/home/user/.build/toolchain/arm-webos-linux-gnueabi_sdk-buildroot}"
SDL="${NOVA_WEBOS_SDL:-/home/user/.build/sdl/sdl2}"
TARGET=armv7-unknown-linux-gnueabi

command -v cargo >/dev/null || { echo "build-webos: нет cargo в PATH" >&2; exit 1; }
[ -x "$NDK/bin/arm-webos-linux-gnueabi-gcc" ] || { echo "build-webos: нет NDK в $NDK" >&2; exit 1; }

export PATH="$NDK/bin:$PATH"

# SDL-webOS кладём в sysroot тулчейна: обёртка gcc сама добавит префикс
# sysroot к абсолютным путям -L/-I, поэтому prefix=/usr/local в sdl2.pc
# оставляем как есть.
SYSROOT="$NDK/arm-webos-linux-gnueabi/sysroot"
mkdir -p "$SYSROOT/usr/local/include"
cp -a "$SDL/lib" "$SYSROOT/usr/local/" 2>/dev/null || true
cp -a "$SDL/include/SDL2" "$SYSROOT/usr/local/include/" 2>/dev/null || true
export PKG_CONFIG_PATH="$SDL/lib/pkgconfig"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR=""

echo "==> cargo build --target $TARGET --features webos"
cargo build --release \
    --target "$TARGET" \
    --config "$ROOT/tools/cargo-webos.toml" \
    --features webos \
    --manifest-path "$ROOT/Cargo.toml" \
    -p nova-launcher

BINARY="$ROOT/target/$TARGET/release/nova"
echo "==> strip"
arm-webos-linux-gnueabi-strip -s "$BINARY"
file "$BINARY" || true

echo "==> package.py"
python3 "$ROOT/tools/package.py" --binary "$BINARY" --tv "$ROOT/tv" --out "$ROOT/dist" --arch arm

IPK="$(ls -1 "$ROOT/dist"/com.novaos.launcher_*.ipk | head -1)"
echo "==> repository.py"
python3 "$ROOT/tools/repository.py" --ipk "$IPK" \
    --base-url "${NOVA_REPO_URL:-https://novaos.example.org/repo}" \
    --out "$ROOT/dist"

echo "==> проверка своим же парсером"
cargo run --release --manifest-path "$ROOT/Cargo.toml" -p nova-core --example inspect -- "$IPK"

echo "Готово: $ROOT/dist"
ls -la "$ROOT/dist"
