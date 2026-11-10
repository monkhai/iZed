#!/usr/bin/env python3
"""Check that APK packaging preserves the exact bundled SSH server archives."""
import argparse
import hashlib
from pathlib import Path
import zipfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("apk", type=Path)
parser.add_argument("archives", type=Path)
args = parser.parse_args()
with zipfile.ZipFile(args.apk) as apk:
    assets = [name for name in apk.namelist() if name.startswith("assets/RemoteServers/") and not name.endswith("/")]
    assert assets, "APK has no remote server archives"
    for asset in assets:
        name = Path(asset).name
        assert name in ("macos-arm64.gz.bin", "macos-x86_64.gz.bin"), f"Unexpected or transformed asset: {name}"
        original = args.archives / name.removesuffix(".bin")
        with apk.open(asset) as packaged, original.open("rb") as source:
            assert hashlib.file_digest(packaged, "sha256").digest() == hashlib.file_digest(source, "sha256").digest(), f"Archive bytes changed: {name}"
        print(f"PASS: {name} matches source gzip")
