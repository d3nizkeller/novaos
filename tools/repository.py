#!/usr/bin/env python3
"""Генерация репозитория NovaOS: ``repo.json`` и ``sha256sums``.

Формат совпадает с домашним репозиторием webosbrew: массив ``packages`` с
манифестом внутри каждой записи, по которому Homebrew Channel и магазин
NovaOS проверяют версию, размер и SHA-256 пакета перед установкой.

Пример::

    python3 tools/repository.py --ipk dist/com.novaos.launcher_0.1.0_arm.ipk \
        --base-url https://novaos.example.org/repo --out dist
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
import io
from pathlib import Path


def read_control(ipk: Path) -> dict:
    """Поля control-файла из ar-архива IPK."""
    blob = ipk.read_bytes()
    assert blob.startswith(b"!<arch>\n"), "это не ar-архив"
    offset = 8
    control: dict[str, str] = {}
    while offset + 60 <= len(blob):
        header = blob[offset : offset + 60].decode("ascii", "replace")
        name = header[0:16].strip()
        size = int(header[48:58].strip())
        payload = blob[offset + 60 : offset + 60 + size]
        if name.startswith("control.tar.gz"):
            with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as tar:
                member = tar.extractfile("./control") or tar.extractfile("control")
                if member is None:
                    raise SystemExit("в control.tar.gz нет файла control")
                for line in member.read().decode("utf-8").splitlines():
                    if ":" in line:
                        key, value = line.split(":", 1)
                        control[key.strip()] = value.strip()
        offset += 60 + size + (size % 2)
    return control


def read_appinfo(ipk: Path) -> dict:
    """appinfo.json приложения из data.tar.gz."""
    blob = ipk.read_bytes()
    offset = 8
    while offset + 60 <= len(blob):
        header = blob[offset : offset + 60].decode("ascii", "replace")
        name = header[0:16].strip()
        size = int(header[48:58].strip())
        payload = blob[offset + 60 : offset + 60 + size]
        if name.startswith("data.tar.gz"):
            with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as tar:
                for member in tar.getmembers():
                    if member.name.endswith("/appinfo.json"):
                        handle = tar.extractfile(member)
                        if handle:
                            return json.loads(handle.read().decode("utf-8"))
        offset += 60 + size + (size % 2)
    raise SystemExit("в data.tar.gz нет appinfo.json")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ipk", type=Path, action="append", required=True, help="IPK пакета (можно несколько)")
    parser.add_argument("--base-url", default="https://novaos.example.org/repo", help="корень репозитория")
    parser.add_argument("--out", type=Path, default=Path("dist"), help="каталог результата")
    parser.add_argument("--webos-release", default="4.0.0", help="минимальный webOS для requirements")
    args = parser.parse_args()

    packages = []
    sums = []
    for ipk in args.ipk:
        if not ipk.exists():
            print(f"repository.py: нет файла {ipk}", file=sys.stderr)
            return 1
        blob = ipk.read_bytes()
        digest = hashlib.sha256(blob).hexdigest()
        size = len(blob)
        control = read_control(ipk)
        appinfo = read_appinfo(ipk)
        file_name = ipk.name
        sums.append(f"{digest}  {file_name}")

        icon_uri = f"{args.base_url.rstrip('/')}/{appinfo['id']}/icon.png"
        packages.append(
            {
                "id": appinfo["id"],
                "title": appinfo.get("title", appinfo["id"]),
                "iconUri": icon_uri,
                "detailIconUri": f"{args.base_url.rstrip('/')}/{appinfo['id']}/largeIcon.png",
                "manifestUrl": f"{args.base_url.rstrip('/')}/repo.json",
                "pool": "main",
                "requirements": {"webosRelease": args.webos_release},
                "screenshots": [],
                "featured": True,
                "shortDescription": appinfo.get("appDescription", ""),
                "fullDescriptionUrl": f"{args.base_url.rstrip('/')}/{appinfo['id']}/README.md",
                "manifest": {
                    "id": appinfo["id"],
                    "version": appinfo["version"],
                    "type": "native",
                    "title": appinfo.get("title", appinfo["id"]),
                    "appDescription": appinfo.get("appDescription", ""),
                    "iconUri": icon_uri,
                    "sourceUrl": "https://github.com/novaos/novaos",
                    "rootRequired": False,
                    "ipkUrl": f"{args.base_url.rstrip('/')}/{file_name}",
                    "ipkHash": {"sha256": digest},
                    "ipkSize": size,
                    "installedSize": int(control.get("Installed-Size", "0")) * 1024,
                },
            }
        )

    args.out.mkdir(parents=True, exist_ok=True)
    repo = {
        "paging": {"page": 1, "count": len(packages), "maxPage": 1, "itemsTotal": len(packages)},
        "packages": packages,
    }
    repo_path = args.out / "repo.json"
    repo_path.write_text(json.dumps(repo, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    sums_path = args.out / "sha256sums"
    sums_path.write_text("\n".join(sums) + "\n", encoding="utf-8")

    print(f"{repo_path}: пакетов — {len(packages)}")
    print(f"{sums_path}: {sums_path.stat().st_size} байт")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
