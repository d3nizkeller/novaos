# Упаковка: IPK, репозиторий, релизы

NovaOS распространяется как IPK-пакет webOS и метаданные репозитория в формате
webosbrew. Всё собирают три скрипта из `tools/`, а рабочие потоки GitHub
повторяют их шаги в CI и релизах.

## Формат IPK

Пакет — ar-архив с тремя участниками:

```
!<arch>
debian-binary      "2.0\n"
control.tar.gz     ./control
data.tar.gz        ./usr/palm/applications/com.novaos.launcher/…
```

`control` (формат webOS 2):

```
Package: com.novaos.launcher
Version: 0.1.1
Section: misc
Priority: optional
Architecture: arm
Installed-Size: 7748119
Maintainer: NovaOS Team <novaos@localhost>
Description: Fast native launcher for LG webOS TV: tiles, store, sideload.
webOS-Package-Format-Version: 2
webOS-Packager-Version: x.y.x
```

`Description` — только ASCII (установщик разбирает control до кодировок
интерфейса), `Installed-Size` — в байтах, как у пакетов webosbrew.

`data.tar.gz` раскладывается в `usr/palm/applications/<id>/`: бинарник `nova`
(0755), `appinfo.json`, иконки, `home.sh`, хук `init.d/50-novaos-home` и
`lib/libSDL2-2.0.so.0` — SDL-webOS едет внутри пакета, потому что в прошивке
webOS её нет; бинарник собирается с RPATH `$ORIGIN/lib` и путями lib всех
каталогов установки (как у заведомо рабочего luma).
Владельцы файлов — uid/gid 1001 (пользователь приложений webOS). Каталоги
`usr`, `usr/palm`, `usr/palm/applications` лежат явными записями с mode 0777,
имена участников — без префикса `./`, формат tar — ustar: так упакованы
заведомо рабочие пакеты webOS, и установщик ожидает именно эту раскладку.

`appinfo.json` объявляет приложение нативным и домашним:

```json
{
  "id": "com.novaos.launcher",
  "main": "nova",
  "type": "native",
  "appCategory": ["home"],
  "requiredACG": ["application.launcher", "application.query", "lunabus.query"]
}
```

Категория `home` и права ACG нужны, чтобы оболочка разрешала подмену Home и
запрос списка приложений.

## tools/package.py

Собирает IPK из бинарника и каталога `tv/`:

```bash
python3 tools/package.py \
    --binary target/armv7-unknown-linux-gnueabi/release/nova \
    --tv tv --out dist --arch arm
```

Скрипт сам считает `Installed-Size`, проставляет права и владельцев, печатает
SHA-256. Результат проверяется собственным парсером:

```bash
cargo run -p nova-core --example inspect -- dist/com.novaos.launcher_0.1.0_arm.ipk
```

## tools/repository.py

Генерирует `repo.json` и `sha256sums` по готовым IPK:

```bash
python3 tools/repository.py --ipk dist/com.novaos.launcher_0.1.0_arm.ipk \
    --base-url https://владелец.github.io/novaos/repo --out dist
```

В манифест пакета попадают версия и описание из `appinfo.json`, размер и
SHA-256 из файла, `requirements.webosRelease` и ссылки на иконки. Магазин
NovaOS и Homebrew Channel читают один и тот же формат, поэтому репозиторий
можно добавлять в оба.

## tools/build-webos.sh

Полный конвейер одним вызовом:

1. кросс-сборка `cargo build --release --target armv7-unknown-linux-gnueabi
   --config tools/cargo-webos.toml --features webos`;
2. SDL-webOS берётся из sysroot тулчейна (`prefix=/usr/local` в `sdl2.pc`
   совпадает с раскладкой sysroot, обёртка gcc добавляет префикс сама);
3. `strip` бинарника тулчейном;
4. `package.py` → IPK;
5. `repository.py` → `repo.json` + `sha256sums`;
6. проверка пакета примером `inspect`.

Особенности целевой платформы: glibc sysroot — 2.12, поэтому крейт содержит
`compat.rs` с реализацией `getauxval` через `/proc/self/auxv` (символ нужен
свежему `std` и `ring`, в glibc 2.12 его нет). Модуль включается только для
`linux + arm` и на desktop-сборках не компилируется.

Пути SDK задаются переменными (по умолчанию — рабочей станцией разработки):

```bash
NOVA_WEBOS_NDK=/путь/arm-webos-linux-gnueabi_sdk-buildroot \
NOVA_WEBOS_SDL=/путь/sdl2 \
NOVA_REPO_URL=https://…/repo \
tools/build-webos.sh
```

`tools/setup-webos-sdk.sh` скачивает тулчейн openlgtv (buildroot-nc4,
webos-b17b4cc) и SDL-webOS 2.30.12-webos.5 в `~/.cache/novaos`.

## CI и релизы

- `.github/workflows/ci.yml`: тесты воркспейса, тесты с SDL, `cargo fmt
  --check`, рендер кадров offscreen и сухой прогон упаковки с проверкой
  парсером.
- `.github/workflows/release.yml` (тег `v*`): кросс-сборка, IPK и метаданные в
  артефакты релиза, затем публикация репозитория в GitHub Pages
  (`/repo/repo.json`, `/repo/*.ipk`, иконки и README пакета).

После публикации репозитория адрес `https://<owner>.github.io/<repo>/repo/repo.json`
можно добавлять в NovaOS (**Настройки → Добавить репозиторий**) и в Homebrew
Channel — обновления пользователи получат автоматически.
