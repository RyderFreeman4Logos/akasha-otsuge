#!/usr/bin/env python3
"""Committed WAL and native value-log corruption must fail without silent repair."""
import json
import os
import pathlib
import selectors
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'helix'))
from run_case import digest, process_identity, publish


def main():
    binary, output = pathlib.Path(sys.argv[1]).resolve(strict=True), pathlib.Path(sys.argv[2]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 authority corruption receipts\n')
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-corruption-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 fallback database\n')
        publish(output / 'admission.json', dict(binary_sha256=digest(binary),
            harness_sha256=digest(pathlib.Path(__file__)), temporary_root=temporary,
            controller=process_identity(os.getpid()), fault='flip one known committed value byte in owned WAL'))
        with (output / 'commit.stdout').open('xb') as stdout:
            child = subprocess.Popen([str(binary), str(root), 'write', 'after-reply'],
                stdout=stdout, stderr=subprocess.PIPE,
                env=dict(os.environ, AKASHA_SPIKE_PROFILE='low'))
            identity = process_identity(child.pid)
            prefix = b''
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(child.stderr, selectors.EVENT_READ)
                    until = time.monotonic() + 60
                    while time.monotonic() < until:
                        assert child.poll() is None, prefix
                        if selector.select(timeout=1):
                            prefix += os.read(child.stderr.fileno(), 4096)
                            if b'PROBE_CHECKPOINT:after-reply\n' in prefix:
                                break
                    else:
                        raise TimeoutError('committed reply absent')
                assert process_identity(child.pid)['start_ticks'] == identity['start_ticks']
                child.kill()
                _, tail = child.communicate(timeout=10)
                (output / 'commit.stderr').write_bytes(prefix + tail)
                assert child.returncode == -9 and process_identity(child.pid) is None
            finally:
                if child.poll() is None:
                    child.kill()
                child.communicate(timeout=10)
        assert json.loads((output / 'commit.stdout').read_text()) == dict(committed=True, reused=False)
        logs = [p for p in root.rglob('*.wal') if b'one' in p.read_bytes()]
        assert len(logs) == 1, [str(p) for p in logs]
        wal = logs[0]
        original = wal.read_bytes()
        offset = original.index(b'one')
        damaged = original[:offset] + bytes([original[offset] ^ 1]) + original[offset+1:]
        (output / 'original.wal').write_bytes(original)
        (output / 'damaged.wal').write_bytes(damaged)
        with wal.open('r+b') as target:
            target.write(damaged); target.flush(); os.fsync(target.fileno())
        before = digest(wal)
        reopened = subprocess.run([str(binary), str(root), 'read'], capture_output=True,
                                  timeout=60, env=dict(os.environ, AKASHA_SPIKE_PROFILE='low'))
        (output / 'reopen.stdout').write_bytes(reopened.stdout)
        (output / 'reopen.stderr').write_bytes(reopened.stderr)
        assert reopened.returncode == 1 and b'WalCorruption' in reopened.stderr, reopened
        assert not reopened.stdout and digest(wal) == before, 'corruption was silently repaired'
        result = dict(status='passed', binary_sha256=digest(binary), committed_child=identity,
            kill_exit=-9, reopen_exit=reopened.returncode, wal_relative=str(wal.relative_to(root)),
            flipped_offset=offset, damaged_wal_unchanged=True,
            limitation='small WAL process-crash fixture; no power-loss or full-size corruption proof')
        large = root / 'large-value'
        large.mkdir(mode=0o700)
        (large / 'OWNER').write_text('akasha-otsuge MVP-003 fallback database\n')
        command = [str(binary), str(large)]
        env = dict(os.environ, AKASHA_SPIKE_PROFILE='low')
        committed = subprocess.run([*command, 'disk-full'], input=b'go\n',
                                   capture_output=True, timeout=60, env=env)
        (output / 'large-commit.stdout').write_bytes(committed.stdout)
        (output / 'large-commit.stderr').write_bytes(committed.stderr)
        assert committed.returncode == 0 and json.loads(committed.stdout) == dict(committed=True)
        before_read = subprocess.run([*command, 'read'], capture_output=True, timeout=60, env=env)
        assert before_read.returncode == 0 and json.loads(before_read.stdout)['values'] == ['x' * 16384] * 5
        (output / 'large-before.stdout').write_bytes(before_read.stdout)
        value_logs = [p for p in large.rglob('*.vlog') if b'x' * 16384 in p.read_bytes()]
        assert len(value_logs) == 1, value_logs
        value_log = value_logs[0]
        original = value_log.read_bytes()
        offset = original.index(b'x' * 16384) + 128
        damaged = original[:offset] + bytes([original[offset] ^ 1]) + original[offset+1:]
        (output / 'original.vlog').write_bytes(original)
        (output / 'damaged.vlog').write_bytes(damaged)
        with value_log.open('r+b') as target:
            target.write(damaged); target.flush(); os.fsync(target.fileno())
        before = digest(value_log)
        reopened = subprocess.run([*command, 'read'], capture_output=True, timeout=60, env=env)
        (output / 'large-reopen.stdout').write_bytes(reopened.stdout)
        (output / 'large-reopen.stderr').write_bytes(reopened.stderr)
        assert reopened.returncode == 1 and b'Key+Value CRC32 mismatch' in reopened.stderr, reopened
        assert not reopened.stdout and digest(value_log) == before
        result['value_log'] = dict(relative=str(value_log.relative_to(large)),
            flipped_offset=offset, reopen_exit=reopened.returncode, damaged_value_log_unchanged=True)
    result['temporary_root_removed'] = not pathlib.Path(temporary).exists()
    result['artifact_sha256'] = {p.name:digest(p) for p in output.iterdir() if p.is_file()}
    publish(output / 'result.json', result)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
