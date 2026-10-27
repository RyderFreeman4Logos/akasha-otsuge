#!/usr/bin/env python3
"""Reconcile a real native effect after controller and child process crashes."""
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time

from run_case import digest, publish


def main():
    binary, source, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    runner = pathlib.Path(__file__).with_name('run_generation.py').resolve()
    reconcile = runner.with_name('reconcile_generation.py')
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 generation abandonment evidence\n')
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-generation-abandon-') as temporary:
        root = pathlib.Path(temporary)
        (root/'OWNER').write_text('akasha-otsuge MVP-003 disposable abandonment database\n')
        database = root/'database'
        publish(output/'admission.json', dict(binary_sha256=digest(binary), temporary_root=temporary,
            source_sha256={str(p):digest(p) for p in [runner,reconcile,pathlib.Path(__file__)]}))

        def run(name, args, success=True):
            result = subprocess.run(args, capture_output=True, text=True, timeout=180)
            (output/(name+'.log')).write_text(result.stdout+result.stderr)
            records.append(dict(name=name, command=args, exit=result.returncode))
            publish(output/'progress.json', dict(completed=len(records), total=12, phase=name, elapsed_s=time.monotonic()-started))
            assert (result.returncode == 0) == success, (name, result.stderr)
            return result.stdout

        def config(name, request):
            p, c = output/(name+'.json'), output/(name+'.toml')
            p.write_text(json.dumps(request))
            c.write_text(f'''[run]
id = "{name}"
profile = "low"
seconds = 180
[paths]
binary = {json.dumps(str(binary))}
database = {json.dumps(str(database))}
request = {json.dumps(str(p))}
output = {json.dumps(str(output/name))}
[disk]
start_bytes = 81212254720
reserve_bytes = 36212254720
[reliability]
max_attempts = 1
''')
            return c

        run('load', [str(binary), str(database), '--load', str(source/'input.jsonl'), str(source/'manifest.json')])
        begin = json.loads((source/'begin.json').read_text());begin['manifest'] = str(source/'manifest.json')
        run('begin', [sys.executable, str(runner), str(config('begin-run', begin))])
        conf = config('interrupted', dict(op='backfill', generation=1, waves=1, hold='after-backfill-commit'))
        command = [sys.executable, str(reconcile), str(output/'interrupted'), '--abandon', 'interrupted']
        with (output/'controller.log').open('x') as log:
            child = subprocess.Popen([sys.executable, str(runner), str(conf)], stdout=log, stderr=subprocess.STDOUT)
            attempt = None
            try:
                deadline = time.monotonic()+60
                while time.monotonic() < deadline:
                    state = output/'interrupted/state.json'
                    if state.exists():
                        value = json.loads(state.read_text())
                        if value['attempts']:
                            attempt = value['attempts'][-1]
                            p = pathlib.Path(attempt['log'])
                            if p.exists() and 'PROBE_CHECKPOINT:after-backfill-commit' in p.read_text():
                                execution = next(json.loads(line) for line in p.with_name('memory.jsonl').read_text().splitlines() if json.loads(line)['event']=='executable')
                                break
                    assert child.poll() is None
                    time.sleep(.05)
                else:raise TimeoutError('native durable checkpoint absent')
                run('wrong-id', command[:-1]+['wrong'], False)
                run('live-controller', command, False)
                child.kill();assert child.wait(timeout=10) == -9
                run('live-native', command, False)
                os.kill(execution['pid'], signal.SIGKILL)
                subprocess.run(['systemctl','--user','stop',attempt['unit']], check=True, timeout=60)
            finally:
                if child.poll() is None:child.kill();child.wait(timeout=10)
                if attempt:subprocess.run(['systemctl','--user','stop',attempt['unit']], check=False, capture_output=True, timeout=60)
        state = output/'interrupted/state.json'
        before = {str(p.relative_to(state.parent)):digest(p) for p in state.parent.rglob('*') if p.is_file() and p != state}
        receipt = json.loads(run('abandon', command))
        assert receipt['effects'] == 'unknown' and receipt['status'] == 'failed'
        assert before == {str(p.relative_to(state.parent)):digest(p) for p in state.parent.rglob('*') if p.is_file() and p != state}
        saved = digest(state)
        run('reuse-abandonment', command);assert digest(state) == saved
        run('abandoned-resume-refused', [sys.executable,str(runner),str(conf),'--resume'], False)
        successor = config('successor', dict(op='backfill', generation=1, waves=1))
        run('successor', [sys.executable,str(runner),str(successor)])
        run('completed-refused', [sys.executable,str(reconcile),str(output/'successor'),'--abandon','successor'], False)
        run('old-owner-refused', command, False)
        count = json.loads(run('coverage', [str(binary),str(database),'MATCH (d:Derived1Text) RETURN count(d)']).splitlines()[-1])
        assert count['rows'] == [[8]] and len(records) == 12
    publish(output/'result.json', dict(status='passed', records=records, seconds=time.monotonic()-started,
        retained_artifacts_unchanged=True, native_effect_rows=8, temporary_database_removed=True, not_tested=['full corpus','power loss']))
    print(json.dumps(dict(status='passed', checks=len(records), seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
