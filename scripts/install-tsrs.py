#!/usr/bin/env python3
"""Install the pinned native tsrs checker and its declaration libraries, without Node."""
import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
VERSION = (ROOT / 'TSRS_VERSION').read_text().strip()


def package_name(oracle=False):
    arch = {'x86_64': 'x64', 'aarch64': 'arm64', 'arm64': 'arm64'}.get(platform.machine())
    if platform.system() == 'Darwin' and arch == 'arm64':
        if oracle:
            return '@typescript/typescript-darwin-arm64'
        return '@tsrs/darwin-arm64'
    if platform.system() == 'Linux' and arch:
        if oracle:
            return f'@typescript/typescript-linux-{arch}'
        libc = 'musl' if platform.libc_ver()[0] == 'musl' or list(Path('/lib').glob('ld-musl-*.so.1')) else 'gnu'
        return f'@tsrs/linux-{arch}-{libc}'
    raise RuntimeError(f'No pinned tsrs binary for {platform.system()} {platform.machine()}')


def download(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def install(prefix, oracle=False):
    version = (ROOT / 'TSC_ORACLE_VERSION').read_text().strip() if oracle else VERSION
    relative_binary = Path('lib/tsc') if oracle else Path('tsrs')
    relative_lib = Path('lib/lib.d.ts') if oracle else Path('lib.d.ts')
    destination = prefix / version
    if destination.exists():
        binary = destination / relative_binary
        if not binary.is_file() or not (destination / relative_lib).is_file():
            raise RuntimeError(f'Incomplete existing install at {destination}; move it aside before reinstalling')
        subprocess.run([str(binary), '--version'], check=True)
        print(binary)
        return
    name = package_name(oracle)
    metadata = json.loads(download(f'https://registry.npmjs.org/{name}/{version}'))
    archive = download(metadata['dist']['tarball'])
    algorithm, encoded = metadata['dist']['integrity'].split('-', 1)
    if algorithm != 'sha512' or hashlib.sha512(archive).digest() != base64.b64decode(encoded):
        raise RuntimeError('Native tsrs archive failed its npm SHA-512 integrity check')
    prefix.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix='.install-', dir=prefix))
    try:
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as package:
            for member in package:
                entry = Path(member.name)
                if not member.isfile() or not entry.parts or entry.parts[0] != 'package':
                    continue
                relative = Path(*entry.parts[1:])
                if relative.parent not in (Path('.'), Path('lib') if oracle else Path('.')):
                    continue
                filename = entry.name
                if not (relative == relative_binary or filename.startswith('lib.') and filename.endswith('.d.ts')
                        or filename.startswith(('LICENSE', 'NOTICE')) or filename == 'package.json'):
                    continue
                source = package.extractfile(member)
                if source is None:
                    raise RuntimeError(f'Cannot read {member.name}')
                (temporary / relative).parent.mkdir(parents=True, exist_ok=True)
                installed_file = temporary / relative
                installed_file.write_bytes(source.read())
                installed_file.chmod(0o644)
        binary = temporary / relative_binary
        if not binary.is_file() or not (temporary / relative_lib).is_file():
            raise RuntimeError('The native package is missing its executable or default libraries')
        binary.chmod(0o755)
        subprocess.run([str(binary), '--version'], check=True)
        # System installs must also work for Bext's isolated build users.
        for directory in temporary.rglob('*'):
            if directory.is_dir():
                directory.chmod(0o755)
        temporary.chmod(0o755)
        os.rename(temporary, destination)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)
    print(destination / relative_binary)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, default=Path.home() / '.local/share/tsc-rs/tsrs')
    parser.add_argument('--oracle', action='store_true', help='install Microsoft TypeScript for differential tests only')
    args = parser.parse_args()
    prefix = Path.home() / '.local/share/tsc-rs/typescript' if args.oracle and args.prefix == parser.get_default('prefix') else args.prefix
    install(prefix.resolve(), args.oracle)
