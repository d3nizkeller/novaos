#!/bin/sh
# NovaOS: автозапуск лаунчера после загрузки системы
set -e
STATE=/var/lib/novaos-home
mkdir -p "$STATE"
cat > /var/lib/webosbrew/init.d/50-novaos-home <<'HOOK'
#!/bin/sh
# Автозапуск NovaOS (установлен лаунчером)
sleep 8
luna-send -n 1 palm://com.webos.applicationManager/launch '{"id":"com.novaos.launcher"}' >/dev/null 2>&1 || true
HOOK
chmod 755 /var/lib/webosbrew/init.d/50-novaos-home
echo "/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher" > "$STATE/app_dir"
echo "/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova" > "$STATE/executable"
echo "NOVA_AUTOSTART_ENABLED"
