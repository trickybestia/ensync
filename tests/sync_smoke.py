#!/usr/bin/env python3
"""Exercise the CLI, filesystem replica and server RPC with a built binary.

Usage: python tests/sync_smoke.py path/to/ensync[.exe]
A launcher, e.g. wine, can precede the executable path.
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

binary = Path(sys.argv[-1]).resolve()
launcher = sys.argv[1:-1]
windows = binary.suffix.lower() == '.exe'

with tempfile.TemporaryDirectory(prefix='ensync smoke ') as tmp:
    root = Path(tmp)
    server_binary = 'ensync server.exe' if windows else binary.name
    shutil.copy2(binary, root / server_binary)

    def ensync(*args, input_text=None):
        proc = subprocess.run(
            [*launcher, binary, *args], cwd=root, input=input_text,
            capture_output=True, text=True, encoding='utf-8', errors='replace',
            timeout=90,
        )
        assert proc.returncode == 0, (args, proc.returncode, proc.stdout, proc.stderr)
        return proc.stdout + proc.stderr

    for name in ('client one', 'client two', 'ciphertext'):
        (root / name).mkdir()

    def configure(name, client, server, passphrase):
        directory = root / name
        directory.mkdir()
        (directory / 'config.toml').write_text(
            f"[general]\npath = '../{client}'\nserver = '{server}'\n"
            f"server_root = 'shared'\npassphrase = '{passphrase}'\n"
            "[[rules.root.files]]\nmode = 'cud/cud'\n",
            encoding='utf-8',
        )

    configure('first', 'client one', 'path:../ciphertext', 'string:smoke')
    # A second client accesses the same store via the server subprocess.
    server = (
        rf'"..\{server_binary}" server ..\ciphertext' if windows
        else f'"../{server_binary}" server ../ciphertext'
    )
    password = 'echo smoke' if windows else 'printf smoke'
    configure('second', 'client two', f'shell:{server}', f'shell:{password}')

    ensync('key', 'init', 'first')
    ensync('mkdir', 'first', '/shared')
    a = root / 'client one' / 'nested folder' / 'résumé.txt'
    b = root / 'client two' / 'nested folder' / 'résumé.txt'
    a.parent.mkdir()
    a.write_bytes(b'one\x00\r\n')
    ensync('sync', 'first')
    ensync('sync', 'second')
    assert b.read_bytes() == a.read_bytes()
    ensync('sync', 'second')  # Fast path reopens the persisted client state.

    b.write_bytes(b'modified from second client')
    ensync('sync', 'second')
    ensync('sync', 'first')
    assert a.read_bytes() == b.read_bytes() == b'modified from second client'

    shutil.move(a, a.with_name('renamed.txt'))
    ensync('sync', 'first')
    ensync('sync', 'second')
    assert not b.exists()
    assert b.with_name('renamed.txt').read_bytes() == b'modified from second client'

    b.with_name('renamed.txt').unlink()
    ensync('sync', 'second')
    ensync('sync', 'first')
    assert not a.with_name('renamed.txt').exists()

    (root / 'source.bin').write_bytes(b'manual transfer\r\n\xff')
    ensync('put', 'first', 'source.bin', 'manual.bin')
    ensync('get', 'second', '/shared/manual.bin', 'result.bin')
    assert (root / 'result.bin').read_bytes() == (root / 'source.bin').read_bytes()

    local_store = root / 'setup store'
    ensync(
        'setup', '--key', 'string:smoke', 'setup config', 'client one',
        str(local_store if not launcher else local_store.name),
        input_text='yes\n\n\n\n',
    )
    assert (local_store / 'state.sqlite').is_file()
    ensync('sync', 'setup config')

print('CLI setup, sync and RPC: upload, download, rescan, update, rename and delete passed')
