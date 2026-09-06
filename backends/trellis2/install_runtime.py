#!/usr/bin/env python3
"""Install the pinned CPython runtime beside an unchanged TRELLIS dependency env.

Conda still owns the CUDA toolkit and compiled Python dependencies. The runtime
loads that env's site-packages through a recorded .pth file; only CPython and its
standard library come from the checksum-verified standalone archive.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import urllib.request

VERSION = '3.11.16'
VERSION_PARTS = [int(part) for part in VERSION.split('.')]
RELEASE = '20260901'
ARCHIVE = f'cpython-{VERSION}+{RELEASE}-x86_64-unknown-linux-gnu-install_only.tar.gz'
URL = f'https://github.com/astral-sh/python-build-standalone/releases/download/{RELEASE}/{ARCHIVE}'
SHA256 = 'faa0758583a63f14c5eee516af82738403b59c13edda6fc0a21d953febd89eed'


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def python_info(python: Path) -> dict:
    env = dict(os.environ, PYTHONNOUSERSITE='1')
    env.pop('PYTHONPATH', None)
    env.pop('PYTHONHOME', None)
    code = ('import json,sys,sysconfig; print(json.dumps(dict('
            'version=list(sys.version_info[:3]), '
            'purelib=sysconfig.get_path("purelib"))))')
    return json.loads(subprocess.check_output([str(python), '-c', code], env=env, text=True, timeout=30))


def install(libraries: Path, destination: Path) -> Path:
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise ValueError('the pinned TRELLIS runtime supports Linux x86_64 only')
    libraries = libraries.resolve(strict=True)
    info = python_info(libraries / 'bin/python')
    if info['version'][:2] != [3, 11]:
        raise ValueError('TRELLIS native dependencies must use the CPython 3.11 ABI')
    site = Path(info['purelib']).resolve(strict=True)
    if '\n' in str(site) or '\r' in str(site):
        raise ValueError('dependency path cannot contain a newline')
    destination = destination.absolute()
    expected = dict(version=VERSION, release=RELEASE, url=URL, archive_sha256=SHA256,
                    libraries=str(libraries), site_packages=str(site))
    receipt = destination / 'forge-runtime.json'
    pth = destination / 'lib/python3.11/site-packages/forge-trellis-dependencies.pth'
    binary = destination / 'bin/python3.11'
    if destination.exists() or destination.is_symlink():
        if not receipt.is_file():
            raise ValueError(f'{destination} exists without a runtime receipt; choose a new destination')
        observed = json.loads(receipt.read_text())
        if any(observed.get(key) != value for key, value in expected.items()):
            raise ValueError(f'{destination} belongs to a different runtime or dependency env; choose a new destination')
        if (not binary.is_file() or digest(binary) != observed.get('python_sha256')
                or not pth.is_file() or pth.read_text() != str(site) + '\n'):
            raise ValueError(f'{destination} has changed since installation; choose a new destination')
        if python_info(binary)['version'] != VERSION_PARTS:
            raise ValueError(f'{destination} does not run Python {VERSION}')
        return destination

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.forge-runtime-', dir=destination.parent) as temp:
        stage = Path(temp)
        archive = stage / ARCHIVE
        with urllib.request.urlopen(URL, timeout=60) as source, archive.open('wb') as target:
            while chunk := source.read(1024 * 1024):
                target.write(chunk)
        if digest(archive) != SHA256:
            raise ValueError('standalone Python archive SHA256 mismatch; nothing installed')
        with tarfile.open(archive) as bundle:
            bundle.extractall(stage, filter='data')
        runtime = stage / 'python'
        candidate = runtime / 'bin/python3.11'
        if python_info(candidate)['version'] != VERSION_PARTS:
            raise ValueError(f'archive does not run Python {VERSION}; nothing installed')
        link = runtime / 'lib/python3.11/site-packages/forge-trellis-dependencies.pth'
        link.write_text(str(site) + '\n')
        expected['python_sha256'] = digest(candidate)
        (runtime / 'forge-runtime.json').write_text(json.dumps(expected, indent=2) + '\n')
        # Never replace an existing directory, including a concurrently created one.
        destination.mkdir()
        # An interrupted move leaves a visible incomplete install for diagnosis.
        for child in runtime.iterdir():
            child.rename(destination / child.name)
    return destination


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--libraries', required=True, type=Path)
    parser.add_argument('--destination', required=True, type=Path)
    args = parser.parse_args()
    print(install(args.libraries, args.destination))


if __name__ == '__main__':
    main()
