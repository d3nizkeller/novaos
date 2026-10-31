#!/bin/sh
# NovaOS: подмена исполняемого файла LG Home (webOS 5+)
set -e
STATE=/var/lib/novaos-home
APP_DIR="/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher"
NOVA="/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova"
HOME_DIR=/usr/palm/applications/com.webos.app.home
mkdir -p "$STATE"
[ -x "$NOVA" ] || { echo "нет исполняемого файла $NOVA" >&2; exit 1; }
MAIN=$(/usr/bin/node -e "console.log(JSON.parse(require('fs').readFileSync('$HOME_DIR/appinfo.json','utf8')).main)" 2>/dev/null || echo "")
[ -n "$MAIN" ] || { echo "не удалось определить точку входа LG Home" >&2; exit 1; }
TARGET="$HOME_DIR/$MAIN"
echo "$TARGET" > "$STATE/target"
cp -a "$TARGET" "$STATE/original-home" 2>/dev/null || true
umount "$TARGET" 2>/dev/null || true
mount --bind "$NOVA" "$TARGET"
touch "$STATE/enabled"
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/close "{\"id\":\"com.webos.app.home\"}" >/dev/null 2>&1 || true'
sleep 1
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/launch "{\"id\":\"com.webos.app.home\"}" >/dev/null 2>&1 || true'
mkdir -p "$(dirname /var/lib/webosbrew/init.d/50-novaos-home)"
cp "$STATE/boot.sh" /var/lib/webosbrew/init.d/50-novaos-home 2>/dev/null || true
chmod 755 /var/lib/webosbrew/init.d/50-novaos-home 2>/dev/null || true
echo "NOVA_HOME_ENABLED"
