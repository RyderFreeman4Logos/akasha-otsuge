#!/usr/bin/env python3
"""Verify controlled high-to-low reopen around one real, exclusively owned job."""
import json
import pathlib
import subprocess
import sys
import time
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / 'helix'))
from run_case import digest, process_identity, publish
from run_generation import stopped


def main():
    controller, high_path, low_path, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    high, low = [tomllib.loads(p.read_text()) for p in (high_path, low_path)]
    assert high['run']['profile'] == 'high' and low['run']['profile'] == 'low'
    assert high['paths']['database'] == low['paths']['database']
    assert digest(high['paths']['binary']) == digest(low['paths']['binary'])
    request = json.loads(pathlib.Path(high['paths']['request']).read_text())
    assert request == json.loads(pathlib.Path(low['paths']['request']).read_text())
    assert request['op'] == 'catch_up'
    checkpoint = pathlib.Path(high['paths']['database']) / f"generation-{request['generation']}-catch-up.json"
    previous = json.loads(checkpoint.read_text()) if checkpoint.exists() else None
    high_output, low_output = [pathlib.Path(c['paths']['output']) for c in (high, low)]
    assert not high_output.exists() and not low_output.exists()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 owned profile transition receipts\n')
    publish(output / 'admission.json', dict(controller_sha256=digest(controller),
        harness_sha256=digest(__file__), high_config_sha256=digest(high_path),
        low_config_sha256=digest(low_path), binary_sha256=digest(high['paths']['binary']),
        scope='operation-controller admission and controlled reopen; production daemon integration is separate'))
    command = lambda path: [sys.executable, str(controller), str(path)]
    started = time.monotonic()
    high_child, failed = None, True
    with (output / 'high-controller.log').open('x') as log:
        try:
            high_child = subprocess.Popen(command(high_path), stdout=log, stderr=subprocess.STDOUT)
            publish(output / 'high-child.json', process_identity(high_child.pid))
            deadline = None
            while high_child.poll() is None:
                path = high_output / 'progress.json'
                if path.exists():
                    progress = json.loads(path.read_text())
                    native = progress.get('native') or {}
                    if native.get('phase') == 'catch_up' and 0 < native['completed'] < native['total']:
                        identity = process_identity(native['pid'])
                        assert identity and identity['state'] != 'Z' and identity['start_ticks'] == native['start_ticks']
                        deadline = native['deadline_unix_ms']
                        if previous and previous['completed'] < previous['total']:
                            assert deadline == previous['deadline_unix_ms']
                        state = json.loads((high_output / 'state.json').read_text())
                        group = pathlib.Path('/sys/fs/cgroup' + pathlib.Path(f"/proc/{native['pid']}/cgroup").read_text().strip().split('::')[1])
                        assert (group / 'memory.max').read_text().strip() == str(2048 * 1024**2)
                        publish(output / 'budget-request.json', dict(requested_budget_mib=512,
                            effective_budget_mib=2048, draining=True, new_admission='closed',
                            native=identity, original_deadline_ms=deadline, progress=native,
                            unit=state['attempts'][-1]['unit']))
                        break
                time.sleep(.1)
            assert deadline is not None, 'high job completed before an in-flight budget change was observed'
            denied = subprocess.run(command(low_path), capture_output=True, text=True, timeout=30)
            (output / 'denied.stdout').write_text(denied.stdout)
            (output / 'denied.stderr').write_text(denied.stderr)
            assert denied.returncode != 0 and 'BlockingIOError' in denied.stderr
            assert not low_output.exists(), 'denied admission created operation state'
            publish(output / 'admission-denied.json', dict(exit=denied.returncode, output_absent=True))
            while high_child.poll() is None:
                progress = json.loads((high_output / 'progress.json').read_text())
                native = progress.get('native') or {}
                if native.get('deadline_unix_ms') is not None:
                    assert native['deadline_unix_ms'] == deadline, 'reload extended the native deadline'
                publish(output / 'progress.json', dict(phase='draining',
                    requested_budget_mib=512, effective_budget_mib=2048, draining=True,
                    elapsed_s=time.monotonic()-started,
                    **{key: native.get(key) for key in ['completed', 'total', 'rate_per_s', 'eta_s']}))
                time.sleep(.5)
            assert high_child.wait() == 0
            high_state = json.loads((high_output / 'state.json').read_text())
            assert high_state['phase'] == 'completed' and high_state['result']['status'] == 'passed'
            stopped(high_state)
            final = json.loads(checkpoint.read_text())
            assert final['deadline_unix_ms'] == deadline and final['completed'] == final['total']
            checkpoint_sha = digest(checkpoint)
            with (output / 'low-controller.log').open('x') as low_log:
                result = subprocess.run(command(low_path), stdout=low_log, stderr=subprocess.STDOUT,
                    timeout=low['run']['seconds'] + 120)
            assert result.returncode == 0
            low_state = json.loads((low_output / 'state.json').read_text())
            assert low_state['phase'] == 'completed' and low_state['result']['status'] == 'passed'
            stopped(low_state)
            assert digest(checkpoint) == checkpoint_sha, 'completed work was replayed or its deadline changed'
            publish(output / 'result.json', dict(status='passed', requested_budget_mib=512,
                effective_budget_mib=512, draining=False, original_deadline_ms=deadline,
                high_job_completed_naturally=True, new_admission_denied_while_draining=True,
                completed_checkpoint_unchanged=True, seconds=time.monotonic()-started,
                high_output=str(high_output), low_output=str(low_output),
                artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()}))
            failed = False
        finally:
            if failed and high_child is not None and high_child.poll() is None:
                state = json.loads((high_output / 'state.json').read_text())
                unit = state['attempts'][-1]['unit']
                publish(output / 'failure-cleanup.json', dict(reason='fixture assertion failure; not budget-change cancellation', unit=unit))
                subprocess.run(['systemctl', '--user', 'stop', unit], check=True, timeout=60)
                high_child.wait(timeout=120)


if __name__ == '__main__':
    main()
