#!/usr/bin/env python3
"""Real interrupted-cursor resume and no-duplicate-owner checks for the runner."""
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
    binary, source_run = [pathlib.Path(p).resolve(strict=True) for p in sys.argv[1:3]]
    output = pathlib.Path(sys.argv[3]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 generation runner evidence\n')
    runner = pathlib.Path(__file__).with_name('run_generation.py').resolve()
    case_runner = runner.with_name('run_case.py')
    records = []
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-generation-runner-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 disposable generation runner database\n')
        database = root / 'database'
        publish(output / 'admission.json', dict(binary_sha256=digest(binary), runner_sha256=digest(runner), harness_sha256=digest(__file__), temporary_root=temporary))

        def run(name, command, expected=0):
            with (output / (name+'.log')).open('x') as log:
                result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=180)
            assert result.returncode == expected if expected == 0 else result.returncode != 0, name
            records.append(dict(name=name, command=command, exit=result.returncode))
            publish(output / 'progress.json', dict(completed=len(records), total=9, phase=name, elapsed_s=time.monotonic()-started))
            return (output / (name+'.log')).read_text()

        def config(name, request, start_bytes=81_212_254_720):
            p = output / (name+'.json')
            p.write_text(json.dumps(request))
            conf = output / (name+'.toml')
            conf.write_text(f'''[run]
id = "{name}"
profile = "low"
seconds = 180
[paths]
binary = {json.dumps(str(binary))}
database = {json.dumps(str(database))}
request = {json.dumps(str(p))}
output = {json.dumps(str(output / name))}
[disk]
start_bytes = {start_bytes}
reserve_bytes = 36212254720
[reliability]
max_attempts = 1
''')
            return conf

        run('load', [str(binary), str(database), '--load', str(source_run / 'input.jsonl'), str(source_run / 'manifest.json')])
        begin = json.loads((source_run / 'begin.json').read_text())
        begin['manifest'] = str(source_run / 'manifest.json')
        conf = config('begin', begin)
        run('begin-run', [sys.executable, str(runner), str(conf)])
        fill = config('fill', dict(op='backfill', generation=1, waves=2, hold='after-backfill-commit'))
        with (output / 'interrupted-controller.log').open('x') as log:
            controller = subprocess.Popen([sys.executable, str(runner), str(fill)], stdout=log, stderr=subprocess.STDOUT)
            engine_pid = None
            try:
                deadline = time.monotonic() + 60
                while time.monotonic() < deadline:
                    state_path = output / 'fill/state.json'
                    if state_path.exists():
                        state = json.loads(state_path.read_text())
                        if state['attempts']:
                            attempt = state['attempts'][-1]
                            engine_log = pathlib.Path(attempt['log'])
                            if engine_log.exists() and 'PROBE_CHECKPOINT:after-backfill-commit' in engine_log.read_text():
                                events = [json.loads(line) for line in engine_log.with_name('memory.jsonl').read_text().splitlines()]
                                executable = next(e for e in events if e['event'] == 'executable')
                                engine_pid = executable['pid']
                                break
                    assert controller.poll() is None
                    time.sleep(0.05)
                else:
                    raise TimeoutError('native backfill checkpoint not reached')
                controller.kill()
                assert controller.wait(timeout=10) == -9
                run('live-unit-refused', [sys.executable, str(runner), str(fill), '--resume'], expected=1)
                assert 'still live' in (output / 'live-unit-refused.log').read_text()
                os.kill(engine_pid, signal.SIGKILL)
                subprocess.run(['systemctl', '--user', 'stop', attempt['unit']], check=True, timeout=60)
            finally:
                if controller.poll() is None:
                    controller.kill()
                    controller.wait(timeout=10)
                if 'attempt' in locals():
                    subprocess.run(['systemctl', '--user', 'stop', attempt['unit']], check=False, capture_output=True, timeout=60)
        case = (source_run / 'case.toml').read_text()
        prior_database = json.loads((source_run / 'admission.json').read_text())['temporary_root'] + '/database'
        case = case.replace(prior_database, str(database)).replace(str(source_run / 'case'), str(output / 'must-not-start-case'))
        case_path = output / 'blocked-case.toml'
        case_path.write_text(case)
        run('unreconciled-case-refused', [sys.executable, str(case_runner), str(case_path)], expected=1)
        assert 'generation operation must be reconciled' in (output / 'unreconciled-case-refused.log').read_text()
        assert not (output / 'must-not-start-case').exists()
        run('resumed-fill', [sys.executable, str(runner), str(fill), '--resume'])
        completed_hash = digest(output / 'fill/state.json')
        run('completed-reuse', [sys.executable, str(runner), str(fill), '--resume'])
        assert digest(output / 'fill/state.json') == completed_hash
        request_path = output / 'fill.json'
        original = request_path.read_bytes()
        request_path.write_bytes(original + b'\n')
        run('changed-request-refused', [sys.executable, str(runner), str(fill), '--resume'], expected=1)
        request_path.write_bytes(original)
        blocked = config('disk-blocked', dict(op='backfill', generation=1, waves=1), start_bytes=2**63)
        run('disk-refused', [sys.executable, str(runner), str(blocked)], expected=1)
        assert 'insufficient disk' in (output / 'disk-refused.log').read_text()
        assert json.loads((output / 'disk-blocked/state.json').read_text())['attempts'] == []
        result = run('coverage', [str(binary), str(database), 'MATCH (d:Derived1Text) RETURN count(d)'])
        assert json.loads(result.splitlines()[-1])['rows'] == [[8]]
        assert len(records) == 9
    publish(output / 'result.json', dict(status='passed', records=records, seconds=time.monotonic()-started, binary_sha256=digest(binary), process_crash_only=True, not_tested=['native ordinary-write disk admission', 'full-size operation', 'power loss']))
    print(json.dumps(dict(status='passed', checks=len(records), seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
