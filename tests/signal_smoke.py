#!/usr/bin/env python3
"""Run after cargo build --release: test interrupting reconnect and --watch."""
import os
import select
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

binary = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    (root / 'files').mkdir()
    (root / 'config.toml').write_text(
        '[general]\npath = "' + str(root / 'files') + '"\n'
        'server = "path:' + str(root / 'missing-server') + '"\n'
        'server_root = "shared"\npassphrase = "string:smoke"\n'
        '[[rules.root.files]]\nmode = "cud/cud"\n'
    )
    for sig in (signal.SIGINT, signal.SIGTERM):
        proc = subprocess.Popen(
            [binary, 'sync', '--watch', '--reconnect', '60', str(root)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0,
        )
        try:
            # Read until the backoff has actually begun.
            lines = []
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                ready, _, _ = select.select([proc.stderr], [], [], 1)
                if ready:
                    line = proc.stderr.readline()
                    lines.append(line)
                    if b'Attempting reconnect' in line:
                        break
                if proc.poll() is not None:
                    raise AssertionError(f'process exited early: {lines!r}')
            else:
                raise AssertionError(f'reconnect did not start: {lines!r}')
            os.kill(proc.pid, sig)
            _, stderr = proc.communicate(timeout=8)
            assert proc.returncode is not None, 'signal did not interrupt reconnect'
            assert b'Sync interrupted' in b''.join(lines) + stderr
            print(f'{sig.name}: reconnect interrupted; status {proc.returncode}')
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.communicate()

    # Recover a missing server during backoff, then interrupt the new watcher.
    proc = subprocess.Popen(
        [binary, 'sync', '--watch', '--quiescence', '0', '--reconnect', '5', str(root)],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, bufsize=0,
    )
    try:
        deadline = time.monotonic() + 30
        lines = []
        while time.monotonic() < deadline:
            ready, _, _ = select.select([proc.stderr], [], [], 1)
            if ready:
                line = proc.stderr.readline()
                lines.append(line)
                if b'Attempting reconnect' in line:
                    break
            if proc.poll() is not None:
                raise AssertionError(f'exited before reconnect: {lines!r}')
        else:
            raise AssertionError(f'reconnect not started: {lines!r}')
        (root / 'missing-server').mkdir(exist_ok=True)
        subprocess.run([binary, 'key', 'init', str(root)], check=True, capture_output=True)
        subprocess.run([binary, 'mkdir', str(root), '/shared'], check=True, capture_output=True)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            ready, _, _ = select.select([proc.stderr], [], [], 1)
            if ready:
                line = proc.stderr.readline()
                lines.append(line)
                if b'continue to monitor' in line:
                    break
            if proc.poll() is not None:
                raise AssertionError(f'exited after reconnect: {lines!r}')
        else:
            raise AssertionError(f'watch not reconnected: {lines!r}')
        os.kill(proc.pid, signal.SIGINT)
        _, stderr = proc.communicate(timeout=10)
        assert proc.returncode == 0, (proc.returncode, stderr)
        assert b'Sync interrupted' in b''.join(lines) + stderr
        print('SIGINT: new watcher woke after reconnect')
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.communicate()

    # A successful initial sync must leave --watch sleeping until the signal.
    (root / 'server').mkdir()
    config = (root / 'config.toml')
    config.write_text(config.read_text().replace('missing-server', 'server'))
    subprocess.run([binary, 'key', 'init', str(root)], check=True, capture_output=True)
    subprocess.run([binary, 'mkdir', str(root), '/shared'], check=True, capture_output=True)
    for sig in (signal.SIGINT, signal.SIGTERM):
        proc = subprocess.Popen(
            [binary, 'sync', '--watch', '--quiescence', '0', str(root)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, bufsize=0,
        )
        try:
            deadline = time.monotonic() + 40
            lines = []
            while time.monotonic() < deadline:
                ready, _, _ = select.select([proc.stderr], [], [], 1)
                if ready:
                    line = proc.stderr.readline()
                    lines.append(line)
                    if b'continue to monitor' in line:
                        break
                if proc.poll() is not None:
                    raise AssertionError(f'watch exited early: {lines!r}')
            else:
                raise AssertionError(f'watch did not start: {lines!r}')
            os.kill(proc.pid, sig)
            _, stderr = proc.communicate(timeout=10)
            assert proc.returncode == 0, (proc.returncode, stderr)
            assert b'Sync interrupted' in b''.join(lines) + stderr
            print(f'{sig.name}: watch woke and stopped cleanly')
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.communicate()
