#!/usr/bin/env python3
"""Native fallback primitives, including SIGKILL before and after durable commit."""
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
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    output = pathlib.Path(sys.argv[2]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 fallback primitive receipts\n')
    started, records = time.monotonic(), []
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-primitives-') as temporary:
        root = pathlib.Path(temporary)
        publish(output / 'admission.json', dict(binary_sha256=digest(binary),
            harness_sha256=digest(pathlib.Path(__file__)), controller=process_identity(os.getpid()),
            temporary_root=temporary, client_environment={'LANCE_CPU_THREADS':'2', 'AKASHA_SPIKE_PROFILE':'low'},
            scope='small primitives; no scale acceptance'))

        def invoke(case, operation, *, kill=None, failure=None):
            database = root / case
            database.mkdir(exist_ok=True)
            (database / 'OWNER').write_text('akasha-otsuge MVP-003 fallback database\n')
            name = f'{len(records):03d}-{case}-{operation}'
            command = [str(binary), str(database), operation, *([kill] if kill else [])]
            with (output / (name + '.stdout')).open('x') as stdout, (output / (name + '.stderr')).open('xb') as stderr:
                child = subprocess.Popen(command, stdout=stdout, stderr=subprocess.PIPE,
                    env={**os.environ, 'LANCE_CPU_THREADS':'2', 'AKASHA_SPIKE_PROFILE':'low'})
                identity = process_identity(child.pid)
                try:
                    if kill:
                        captured = b''
                        with selectors.DefaultSelector() as selector:
                            selector.register(child.stderr, selectors.EVENT_READ)
                            deadline = time.monotonic() + 60
                            while time.monotonic() < deadline:
                                assert child.poll() is None, (name, captured)
                                if selector.select(timeout=1):
                                    captured += os.read(child.stderr.fileno(), 4096)
                                    if ('PROBE_CHECKPOINT:' + kill + '\n').encode() in captured:
                                        break
                            else:
                                raise TimeoutError(name)
                        actual = process_identity(child.pid)
                        assert actual and actual['start_ticks'] == identity['start_ticks']
                        child.kill()
                        _, tail = child.communicate(timeout=10)
                        stderr.write(captured + tail)
                        assert child.returncode == -9
                    else:
                        _, captured = child.communicate(timeout=180)
                        stderr.write(captured)
                        assert child.returncode == (1 if failure else 0), (name, captured[-3000:])
                        if failure:
                            assert failure.encode() in captured, (name, captured[-3000:])
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=10)
            assert process_identity(child.pid) is None
            if kill:
                reply = (output / (name + '.stdout')).read_text()
                if kill == 'after-reply':
                    assert json.loads(reply) == dict(committed=True, reused=False)
                else:
                    assert not reply, 'reply escaped before the durable crash barrier'
            record = dict(name=name, command=command, child=identity, exit=child.returncode,
                checkpoint=kill, expected_failure=failure)
            records.append(record)
            elapsed = time.monotonic() - started
            publish(output / 'progress.json', dict(phase=name, completed=len(records), total=20,
                elapsed_s=elapsed, rate_per_s=len(records)/elapsed,
                eta_s=max(0, 20-len(records))*elapsed/len(records)))
            if kill or failure:
                return None
            return json.loads((output / (name + '.stdout')).read_text().splitlines()[-1])

        assert invoke('empty', 'read')['values'] == [None] * 5
        for phase in ['before-commit', 'after-commit', 'after-reply']:
            invoke(phase, 'write', kill=phase)
            expected = [None] * 5 if phase == 'before-commit' else ['one'] * 5
            assert invoke(phase, 'read')['values'] == expected
        assert invoke('rollback', 'rollback')['rolled_back']
        assert invoke('rollback', 'read')['values'] == [None] * 5
        assert invoke('conflict', 'conflict')['write_conflict']
        assert invoke('conflict', 'read')['values'] == ['one'] * 5
        assert invoke('history', 'history-write')['versions'] == 2
        history = invoke('history', 'history-read')
        assert history == dict(current='two', at_one='one', at_two='two')
        invoke('empty', 'unknown-operation', failure='unknown operation')
        search = invoke('search', 'search-create')
        assert search == dict(rows=32, ann_id=0, text_id=0)
        assert invoke('search', 'search-read') == search
        assert invoke('after-commit', 'write')['reused']
        assert invoke('after-commit', 'read')['values'] == ['one'] * 5
        invoke('search', 'search-create', failure='TableAlreadyExists')
        assert invoke('search', 'search-read') == search
        assert len(records) == 20, len(records)
    elapsed = time.monotonic() - started
    publish(output / 'progress.json', dict(phase='completed', completed=20, total=20,
        elapsed_s=elapsed, rate_per_s=20/elapsed, eta_s=0))
    publish(output / 'result.json', dict(status='passed', seconds=elapsed, records=records,
        artifact_sha256={str(p.relative_to(output)):digest(p) for p in output.rglob('*') if p.is_file()},
        not_tested=['power loss', 'full-scale memory', 'large index construction', 'cross-store outbox']))
    print(json.dumps(dict(status='passed', checks=len(records), seconds=elapsed)))


if __name__ == '__main__':
    main()
