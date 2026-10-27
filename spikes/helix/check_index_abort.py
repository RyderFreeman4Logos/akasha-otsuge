#!/usr/bin/env python3
"""Abort an older probe's blocked native build through the owned controller."""
import json
import pathlib
import subprocess
import sys
import tempfile
import time

from run_case import digest, publish


def main():
    binary, previous, source, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    runner = pathlib.Path(__file__).with_name('run_generation.py').resolve()
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 index abort evidence\n')
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-index-abort-') as temporary:
        root = pathlib.Path(temporary)
        (root/'OWNER').write_text('akasha-otsuge MVP-003 disposable abort database\n')
        database = root/'database'
        publish(output/'admission.json', dict(binary_sha256=digest(binary), previous_sha256=digest(previous),
            temporary_root=temporary, runner_sha256=digest(runner), harness_sha256=digest(__file__)))

        def run(name, command, success=True):
            result = subprocess.run(command, capture_output=True, text=True, timeout=180)
            (output/(name+'.log')).write_text(result.stdout+result.stderr)
            records.append(dict(name=name, command=command, exit=result.returncode))
            publish(output/'progress.json', dict(completed=len(records), total=10, phase=name, elapsed_s=time.monotonic()-started))
            assert (result.returncode == 0) == success, (name, result.stderr)
            return result

        run('load', [str(previous),str(database),'--load',str(source/'input.jsonl'),str(source/'manifest.json')])
        run('duplicate', [str(previous),str(database),'MATCH (d:Document) SET d.memory = 1000001'])
        status = output/'status.json';publish(status, dict(op='status'))
        blocked = run('blocked-build', [str(previous),str(database),'--generation',str(status)], False)
        receipt = [json.loads(line.split(':',1)[1]) for line in blocked.stderr.splitlines() if line.startswith('INDEX_PROGRESS:')][-1]
        assert receipt['operation']['status'] == 'blocked'
        operation = receipt['operation']['operation_id']
        request = output/'abort.json';publish(request, dict(op='abort_index', generation=1, operation_id=operation))
        config = output/'abort.toml'
        config.write_text(f'''[run]
id = "abort"
profile = "low"
seconds = 180
[paths]
binary = {json.dumps(str(binary))}
database = {json.dumps(str(database))}
request = {json.dumps(str(request))}
output = {json.dumps(str(output/'abort'))}
[disk]
start_bytes = 81212254720
reserve_bytes = 36212254720
[reliability]
max_attempts = 1
''')
        for i, invalid in enumerate(['', 'not-an-id', '00000000-0000-0000-0000-00000000000z']):
            absent = root/'must-not-open'
            run('invalid-'+str(i), [str(binary),str(absent),'--abort-index',invalid], False)
            assert not absent.exists()
        publish(request, dict(op='abort_index', generation=1, operation_id='invalid'))
        run('controller-invalid', [sys.executable,str(runner),str(config)], False)
        assert not (output/'abort').exists()
        publish(request, dict(op='abort_index', generation=1, operation_id=operation))
        run('abort', [sys.executable,str(runner),str(config)])
        before = {str(p.relative_to(output/'abort')):digest(p) for p in (output/'abort').rglob('*') if p.is_file()}
        run('reuse', [sys.executable,str(runner),str(config),'--resume'])
        assert before == {str(p.relative_to(output/'abort')):digest(p) for p in (output/'abort').rglob('*') if p.is_file()}
        count = run('authority-preserved', [str(binary),str(database),'MATCH (d:Document) RETURN count(d)'])
        assert json.loads(count.stdout)['rows'] == [[8]]
        terminal = json.loads((database/'index-abort-progress.json').read_text())
        assert terminal['status']['operation']['status'] == 'aborted'
        assert len(records) == 10
    publish(output/'result.json', dict(status='passed', records=records, seconds=time.monotonic()-started,
        operation_id=operation, terminal=terminal, temporary_database_removed=True,
        not_tested=['full-size abort latency','power loss']))
    print(json.dumps(dict(status='passed', checks=len(records), seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
