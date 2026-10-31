#!/usr/bin/env python3
"""Упаковка NovaOS в IPK-пакет webOS.

Пакет — это ar-архив с тремя участниками: ``debian-binary`` ("2.0\\n"),
``control.tar.gz`` и ``data.tar.gz``. Данные раскладываются в
``usr/palm/applications/<id>/`` — оттуда webOS запускает нативные приложения.

Пример::

    python3 tools/package.py \
        --binary target/armv7-unknown-linux-gnueabi/release/nova \
        --tv tv --out dist
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import stat
import sys
import tarfile
import time
from pathlib import Path

UID_GID = 1001  # пользователь приложений webOS
APP_ID = "com.novaos.launcher"


def ar_member(name: str, data: bytes, mtime: int) -> bytes:
    """Один участник ar-архива в формате GNU/debian."""
    header = (
        f"{name:<16}"  # имя
        f"{mtime:<12}"  # mtime
        f"{0:<6}"  # uid
        f"{0:<6}"  # gid
        f"{100644:<8}"  # mode
        f"{len(data):<10}"  # size
        "`\n"
    )
    assert len(header) == 60, len(header)
    out = header.encode("ascii") + data
    if len(data) % 2:
        out += b"\n"
    return out


def tar_gz(entries: list[tuple[str, bytes, int]], mtime: int) -> bytes:
    """gzip-тар из списка (путь, данные, mode).

    Формат ustar без префиксов ``./`` — ровно так упакованы заведомо
    рабочие пакеты webOS (luma, hbchannel): установщик webOS ожидает
    имена участников вида ``usr/palm/applications/<id>/…`` и явные
    записи каталогов.
    """
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        for path, data, mode in entries:
            info = tarfile.TarInfo(path)
            info.size = len(data)
            info.mtime = mtime
            info.uid = UID_GID
            info.gid = UID_GID
            if mode & 0o40000:
                info.type = tarfile.DIRTYPE
                mode &= 0o7777
            info.mode = mode
            info.uname = "developer"
            info.gname = "developer"
            tar.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def dir_entries(app_dir: str) -> list[tuple[str, bytes, int]]:
    """Явные записи каталогов, как в пакетах webOS."""
    return [
        ("usr", b"", 0o40777),
        ("usr/palm", b"", 0o40777),
        ("usr/palm/applications", b"", 0o40777),
        (app_dir, b"", 0o40755),
    ]


DESCRIPTION = "Fast native launcher for LG webOS TV: tiles, store, sideload."


def control_text(appinfo: dict, installed_bytes: int, architecture: str) -> str:
    """Поля control-файла пакета (формат webOS 2).

    Описание — только ASCII: установщик webOS разбирает control до
    кодировок интерфейса. Размер — в байтах, как у пакетов webosbrew.
    """
    lines = [
        f"Package: {appinfo['id']}",
        f"Version: {appinfo['version']}",
        "Section: misc",
        "Priority: optional",
        f"Architecture: {architecture}",
        f"Installed-Size: {installed_bytes}",
        "Maintainer: NovaOS Team <novaos@localhost>",
        f"Description: {DESCRIPTION}",
        "webOS-Package-Format-Version: 2",
        "webOS-Packager-Version: x.y.x",
    ]
    return "\n".join(lines) + "\n"


def build_ipk(binary: Path, tv: Path, out_dir: Path, architecture: str) -> Path:
    appinfo = json.loads((tv / "appinfo.json").read_text(encoding="utf-8"))
    app_dir = f"usr/palm/applications/{appinfo['id']}"

    binary_bytes = binary.read_bytes()
    entries: list[tuple[str, bytes, int]] = dir_entries(app_dir) + [
        (f"{app_dir}/nova", binary_bytes, 0o755),
        (f"{app_dir}/appinfo.json", (tv / "appinfo.json").read_bytes(), 0o644),
        (f"{app_dir}/icon.png", (tv / "icon.png").read_bytes(), 0o644),
        (f"{app_dir}/largeIcon.png", (tv / "largeIcon.png").read_bytes(), 0o644),
        (f"{app_dir}/home.sh", (tv / "home.sh").read_bytes(), 0o755),
        (f"{app_dir}/init.d", b"", 0o40755),
        (f"{app_dir}/init.d/50-novaos-home", (tv / "init.d" / "50-novaos-home").read_bytes(), 0o755),
    ]
    data = tar_gz(entries, mtime=int(time.time()))
    installed_bytes = len(binary_bytes) + sum(len(item[1]) for item in entries)

    control = control_text(appinfo, installed_bytes, architecture).encode("ascii")
    control_tar = tar_gz([("control", control, 0o666)], mtime=int(time.time()))

    mtime = int(time.time())
    archive = b"!<arch>\n"
    archive += ar_member("debian-binary", b"2.0\n", mtime)
    archive += ar_member("control.tar.gz", control_tar, mtime)
    archive += ar_member("data.tar.gz", data, mtime)

    out_dir.mkdir(parents=True, exist_ok=True)
    name = f"{appinfo['id']}_{appinfo['version']}_{architecture}.ipk"
    target = out_dir / name
    target.write_bytes(archive)
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="скомпилированный бинарник nova")
    parser.add_argument("--tv", type=Path, default=Path("tv"), help="каталог tv/ с манифестом и иконками")
    parser.add_argument("--out", type=Path, default=Path("dist"), help="каталог результата")
    parser.add_argument("--arch", default="arm", help="Architecture пакета (arm или all)")
    args = parser.parse_args()

    if not args.binary.exists():
        print(f"package.py: нет бинарника {args.binary}", file=sys.stderr)
        return 1
    ipk = build_ipk(args.binary, args.tv, args.out, args.arch)
    digest = hashlib.sha256(ipk.read_bytes()).hexdigest()
    print(f"{ipk}  ({ipk.stat().st_size} байт)")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
