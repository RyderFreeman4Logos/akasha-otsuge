#!/usr/bin/env python3
"""Observe one resumable native generation operation with exclusive ownership."""
import argparse
import fcntl
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import tomllib
import uuid

from run_case import digest, process_identity, publish

MIN_RESERVE = 30 * 1024**3 + 4_000_000_000
OPERATIONS = {'begin_corpus', 'backfill', 'build', 'catch_up', 'activate'}


def stopped(state):
    actual = process_identity(state['controller']['pid'])
    if actual and actual['state'] != 'Z' and actual['start_ticks'] == state['controller']['start_ticks']:
        raise ValueError('generation controller is still live')
    for attempt in state['attempts']:
        status = subprocess.run(['systemctl', '--user', 'show', attempt['unit'], '-p', 'ActiveState', '--value'], check=True, capture_output=True, text=True).stdout.strip()
        if status not in {'inactive', 'failed'}:
            raise ValueError('generation unit is still live: ' + status)


def observe(database, generation, attempt):
    for name in [f'generation-{generation}-backfill.json', f'generation-{generation}-catch-up.json']:
        path = database / name
        if path.exists():
            value = json.loads(path.read_text())
            actual = process_identity(value['pid'])
            if actual and actual['state'] != 'Z':
                cgroup = pathlib.Path(f'/proc/{actual["pid"]}/cgroup').read_text()
                if attempt['unit'] + '.service' in cgroup:
                    return value
    log = pathlib.Path(attempt['log'])
    with log.open('rb') as source:
        source.seek(max(0, log.stat().st_size - 65536))
        for line in reversed(source.read().decode(errors='replace').splitlines()):
            if line.startswith('INDEX_PROGRESS:'):
                return {'native_index': json.loads(line[len('INDEX_PROGRESS:'):]), 'counter_note': 'native entity counter can include repeated passes; not unique coverage'}
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config', type=pathlib.Path)
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    config = tomllib.loads(args.config.read_text())
    if set(config) != {'run', 'paths', 'disk', 'reliability'} or config['reliability'] != {'max_attempts': 1}:
        raise ValueError('one observed attempt; no automatic execution retries')
    run, paths, disk = config['run'], config['paths'], config['disk']
    if set(run) != {'id', 'profile', 'seconds'} or set(paths) != {'binary', 'database', 'request', 'output'} or set(disk) != {'start_bytes', 'reserve_bytes'}:
        raise ValueError('unknown or missing operation setting')
    if run['profile'] not in {'low', 'high'} or type(run['seconds']) is not int or not 1 <= run['seconds'] <= 604800:
        raise ValueError('invalid client resource profile or observation duration')
    if any(type(disk[k]) is not int for k in disk) or not disk['start_bytes'] >= disk['reserve_bytes'] >= MIN_RESERVE:
        raise ValueError('disk plan must retain scratch and 30 GiB free')
    binary, database, request_path = [pathlib.Path(paths[k]).resolve(strict=True) for k in ['binary', 'database', 'request']]
    if request_path.stat().st_size > 1024**2:
        raise ValueError('request exceeds 1 MiB')
    request = json.loads(request_path.read_text())
    if request['op'] not in OPERATIONS or type(request['generation']) is not int or request['generation'] not in {1, 2}:
        raise ValueError('only idempotent retained-generation operations are resumable')
    marker = database.parent / 'OWNER'
    if not marker.is_file() or not marker.read_text().startswith('akasha-otsuge MVP-003 '):
        raise ValueError('database is not an owned storage experiment')
    output = pathlib.Path(paths['output']).resolve()
    lock = (database / '.akasha-case-owner.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    import_lock = None
    if (database.parent / 'controller.lock').exists():
        import_lock = (database.parent / 'controller.lock').open('a')
        fcntl.flock(import_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        imported = json.loads((database.parent / 'state.json').read_text())
        if imported['phase'] != 'completed' or imported['result']['status'] != 'passed':
            raise ValueError('full import must have a verified terminal receipt')
        for name, expected in imported['result']['artifact_sha256'].items():
            if digest(database.parent / name) != expected:
                raise ValueError('completed import artifact changed')
    owner_path = database / '.akasha-generation-owner.json'
    for owner_file in [owner_path, database / '.akasha-case-owner.json']:
        if owner_file.exists():
            owner = json.loads(owner_file.read_text())
            if owner['output'] != str(output):
                prior = json.loads((pathlib.Path(owner['output']) / 'state.json').read_text())
                if prior['phase'] not in {'completed', 'abandoned'}:
                    raise ValueError('prior database operation needs explicit reconciliation')
    observer = pathlib.Path(__file__).resolve().parents[1] / 'measure.py'
    identity = dict(config=config, binary_sha256=digest(binary), request_sha256=digest(request_path), controller_sha256=digest(__file__), observer_sha256=digest(observer), helpers_sha256=digest(pathlib.Path(__file__).with_name('run_case.py')))
    if request['op'] == 'begin_corpus':
        identity['manifest_sha256'] = digest(pathlib.Path(request['manifest']).resolve(strict=True))
    state_path = output / 'state.json'
    if output.exists():
        if not args.resume:
            raise ValueError('existing run requires explicit --resume')
        state = json.loads(state_path.read_text())
        if (state['identity'] != identity or digest(output / 'probe') != identity['binary_sha256']
                or digest(output / 'request.json') != identity['request_sha256']
                or tomllib.loads((output / 'config.toml').read_text()) != config):
            raise ValueError('immutable operation identity changed')
        stopped(state)
        for previous in state['attempts']:
            if previous['phase'] == 'admitted':
                previous.update(phase='interrupted', outcome='controller result absent; native idempotent operation will reconcile durable effects')
        for name, expected in state.get('result', {}).get('artifact_sha256', {}).items():
            if digest(output / name) != expected:
                raise ValueError('completed operation artifact changed: ' + name)
        if state['phase'] == 'completed':
            print(json.dumps(state['result']))
            return 0
    else:
        if args.resume:
            raise ValueError('cannot resume an absent operation')
        output.mkdir(mode=0o700)
        (output / 'OWNER').write_text('akasha-otsuge MVP-003 owned generation operation\n')
        state = dict(identity=identity, attempts=[], phase='admitted', controller=process_identity(os.getpid()), config_epoch=1)
        publish(state_path, state)
        shutil.copy2(binary, output / 'probe')
        shutil.copy2(args.config, output / 'config.toml')
        shutil.copy2(request_path, output / 'request.json')
    if shutil.disk_usage(database).free < disk['start_bytes']:
        raise ValueError('insufficient disk for the admitted generation plan')
    attempt_id = uuid.uuid4().hex
    attempt_dir = output / attempt_id
    attempt_dir.mkdir()
    unit = 'akasha-generation-' + attempt_id
    attempt = dict(id=attempt_id, unit=unit, log=str(attempt_dir / 'engine.log'), phase='admitted', started_unix_s=time.time())
    state['attempts'].append(attempt)
    state.update(phase='running', controller=process_identity(os.getpid()))
    publish(state_path, state)
    publish(owner_path, dict(output=str(output), run_id=run['id']))
    command = ['systemd-run', '--user', '--quiet', '--wait', '--pipe', '--collect', '--unit='+unit, '-p', f'MemoryMax={(640 if run["profile"] == "low" else 2048)*1024**2}', '-p', 'MemorySwapMax=0', '-p', f'RuntimeMaxSec={run["seconds"]}', '-p', 'Environment=AKASHA_SPIKE_PROFILE='+run['profile'], sys.executable, str(observer), str(attempt_dir / 'memory.jsonl'), str(output / 'probe'), str(database), '--generation', str(output / 'request.json')]
    attempt['command'] = command
    publish(state_path, state)
    started, failure = time.monotonic(), None
    with pathlib.Path(attempt['log']).open('x') as log:
        child = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
        try:
            while child.poll() is None:
                free = shutil.disk_usage(database).free
                if free < disk['reserve_bytes']:
                    raise RuntimeError('generation disk reservation reached')
                native = observe(database, request['generation'], attempt)
                counters = {k: (native or {}).get(k) for k in ['completed', 'total', 'rate_per_s', 'eta_s']}
                publish(output / 'progress.json', dict(phase=request['op'], elapsed_s=time.monotonic()-started, native=native, free_bytes=free, reserved_bytes=disk['reserve_bytes'], controller=process_identity(os.getpid()), config_epoch=1, **counters))
                time.sleep(2)
        except BaseException as error:
            failure = str(error) or type(error).__name__
            subprocess.run(['systemctl', '--user', 'stop', unit], check=True, timeout=120)
        finally:
            child.wait(timeout=120)
    try:
        events = [json.loads(line) for line in (attempt_dir / 'memory.jsonl').read_text().splitlines()]
    except (OSError, ValueError) as error:
        events = []
        failure = (failure + '; ' if failure else '') + str(error)
    executions = [row for row in events if row['event'] == 'executable']
    exits = [row for row in events if row['event'] == 'exit']
    limit = str((640 if run['profile'] == 'low' else 2048)*1024**2)
    limits_ok = bool(events) and events[0].get('limits', {}).get('memory.max') == limit and events[0].get('limits', {}).get('memory.swap.max') == '0'
    passed = failure is None and child.returncode == 0 and limits_ok and len(executions) == len(exits) == 1 and executions[0]['sha256'] == identity['binary_sha256'] and exits[0]['code'] == 0
    result = dict(status='passed' if passed else 'failed', failure=failure, exit=child.returncode, elapsed_s=time.monotonic()-started, operation=request['op'], generation=request['generation'])
    artifacts = [output / 'request.json', output / 'config.toml']
    for previous in state['attempts']:
        artifacts.extend([pathlib.Path(previous['log']), pathlib.Path(previous['log']).with_name('memory.jsonl')])
    result['artifact_sha256'] = {str(p.relative_to(output)): digest(p) for p in artifacts if p.exists()}
    attempt.update(phase='completed' if passed else 'failed', result=result)
    state.update(phase='completed' if passed else 'interrupted', result=result)
    publish(state_path, state)
    print(json.dumps(result))
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
