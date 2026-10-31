#!/bin/sh
# NovaOS: подмена каталога LG Home (webOS 3-4, включая UM7300PLB)
set -e
STATE=/var/lib/novaos-home
APP_DIR="/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher"
NOVA="/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova"
HOME_DIR=/usr/palm/applications/com.webos.app.home
mkdir -p "$STATE" "$STATE/app"
[ -x "$NOVA" ] || { echo "нет исполняемого файла $NOVA" >&2; exit 1; }
# Копируем оригинальный Home, чтобы сохранить его манифест, иконки и роль в системе.
cp -a "$HOME_DIR/." "$STATE/app/" 2>/dev/null || true
# Точку входа подменяем на NovaOS; идентификатор приложения остаётся прежним.
/usr/bin/node -e '
  const fs = require("fs");
  const p = process.argv[1] + "/appinfo.json";
  const info = JSON.parse(fs.readFileSync(p, "utf8"));
  info.main = process.argv[2];
  info.handlesRelaunch = false;
  fs.writeFileSync(p, JSON.stringify(info, null, 2));
' "$STATE/app" "$NOVA"
chmod 755 "$NOVA" || true
umount "$HOME_DIR" 2>/dev/null || true
mount --bind "$STATE/app" "$HOME_DIR"
touch "$STATE/enabled"
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/close "{\"id\":\"com.webos.app.home\"}" >/dev/null 2>&1 || true'
sleep 1
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/launch "{\"id\":\"com.webos.app.home\"}" >/dev/null 2>&1 || true'
mkdir -p "$(dirname /var/lib/webosbrew/init.d/50-novaos-home)"
cp "$STATE/boot.sh" /var/lib/webosbrew/init.d/50-novaos-home 2>/dev/null || true
chmod 755 /var/lib/webosbrew/init.d/50-novaos-home 2>/dev/null || true
echo "NOVA_HOME_ENABLED"
