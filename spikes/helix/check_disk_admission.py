#!/usr/bin/env python3
"""Exercise native admission without filling or modifying a shared filesystem."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

from run_case import digest, publish


def main():
    binary, source = [pathlib.Path(p).resolve(strict=True) for p in sys.argv[1:3]]
    output = pathlib.Path(sys.argv[3]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 native disk admission evidence\n')
    started, records = time.monotonic(), []
    with tempfile.TemporaryDirectory(prefix='akasha-disk-admission-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 disposable disk admission database\n')
        database = root / 'database'
        database.mkdir(mode=0o700)
        publish(output / 'admission.json', dict(binary_sha256=digest(binary), harness_sha256=digest(__file__), temporary_root=temporary))

        def run(name, request, *, blocked=False, failure=False, query=None, arguments=None):
            path = output / (name + '.json')
            publish(path, request)
            command = [str(binary), str(database)] + (arguments if arguments else [query] if query else ['--generation', str(path)])
            environment = {**os.environ, 'AKASHA_SPIKE_PROFILE': 'low', 'AKASHA_SPIKE_DISK_RESERVE_BYTES': str(2**63 if blocked else 36212254720)}
            with (output / (name+'.log')).open('x') as log:
                result = subprocess.run(command, env=environment, stdout=log, stderr=subprocess.STDOUT, timeout=180)
            records.append(dict(name=name, exit=result.returncode, command=command, reserve_bytes=environment['AKASHA_SPIKE_DISK_RESERVE_BYTES']))
            elapsed = time.monotonic()-started
            publish(output / 'progress.json', dict(phase=name, completed=len(records), total=26, elapsed_s=elapsed, rate_per_s=len(records)/elapsed, eta_s=(26-len(records))*elapsed/len(records)))
            text = (output / (name+'.log')).read_text()
            assert (result.returncode != 0) == failure, (name, text[-2000:])
            if failure:
                assert 'disk reservation' in text, (name, text[-2000:])
                return None
            return json.loads(text.splitlines()[-1])

        run('initialize', dict(op='status'))
        change = dict(request_id='disk-1', memory=1, expected=0, scope=0, active=True, body='immutable disk admission text', text_only=True)
        run('blocked-commit', dict(op='commit', change=change), blocked=True, failure=True)
        assert run('no-revision', {}, query='MATCH (r:Revision) RETURN count(r)')['rows'] == [[0]]
        receipt = run('commit', dict(op='commit', change=change))
        assert run('reused-commit', dict(op='commit', change=change), blocked=True) == receipt
        assert run('conflict', dict(op='commit', change={**change, 'body':'conflicting text'}), blocked=True) == {'conflict':'idempotency'}
        run('blocked-permission', dict(op='grant', principal='reader', scope=0, allowed=True), blocked=True, failure=True)
        run('permission', dict(op='grant', principal='reader', scope=0, allowed=True))
        current = run('current', dict(op='current', principal='reader', memory=1), blocked=True)
        assert current['entry']['body'] == change['body'], current
        full_request = dict(run_id='disk-full-budget-refused', phase='steady', seconds=1800,
                            fixture=str(source/'queries.json'), output=str(output/'full-refused'),
                            corpus_manifest=str(source/'manifest.json'), diagnostic=False, generation=1)
        run('full-budget-refused', full_request, arguments=['--workload', str(output/'full-budget-refused.json')], failure=True)
        bundle = json.loads((source / 'begin.json').read_text())['bundle']
        begin = dict(op='begin', generation=1, bundle=bundle)
        run('blocked-generation', begin, blocked=True, failure=True)
        run('generation', begin)
        run('blocked-build', dict(op='build', generation=1), blocked=True, failure=True)
        run('build', dict(op='build', generation=1))
        application = dict(op='apply', generation=1, family='text', seq=1)
        run('blocked-application', application, blocked=True, failure=True)
        run('application', application)
        assert run('reused-application', application, blocked=True)['reused']
        run('vector-unneeded', {**application, 'family':'vector'})
        run('blocked-activation', dict(op='activate', generation=1), blocked=True, failure=True)
        run('activation', dict(op='activate', generation=1))
        database = root / 'corpus'
        run('corpus-load', {}, arguments=['--load', str(source / 'input.jsonl'), str(source / 'manifest.json')])
        run('corpus-begin', dict(op='begin_corpus', generation=1, bundle=bundle, manifest=str(source/'manifest.json')))
        run('blocked-backfill', dict(op='backfill', generation=1, waves=2), blocked=True, failure=True)
        assert run('no-derived', {}, query='MATCH (d:Derived1Text) RETURN count(d)')['rows'] == [[0]]
        run('backfill', dict(op='backfill', generation=1, waves=2))
        assert run('corpus-coverage', {}, query='MATCH (d:Derived1Text) RETURN count(d)')['rows'] == [[8]]
    publish(output/'result.json', dict(status='passed', seconds=time.monotonic()-started, records=records, not_tested=['physical disk-full', 'power loss', 'full-size allocation bound']))
    print(json.dumps(dict(status='passed', commands=len(records), seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
