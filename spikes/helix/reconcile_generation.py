#!/usr/bin/env python3
"""Abandon an exact stopped generation run without replaying its effects."""
import argparse
import fcntl
import json
import pathlib
import tomllib

from run_case import digest, publish
from run_generation import stopped


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=pathlib.Path)
    parser.add_argument('--abandon', required=True, metavar='EXACT_RUN_ID')
    args = parser.parse_args()
    output = args.output.resolve(strict=True)
    if (output/'OWNER').read_text() != 'akasha-otsuge MVP-003 owned generation operation\n':
        raise ValueError('foreign generation run')
    state_path = output/'state.json'
    state = json.loads(state_path.read_text())
    identity = state['identity']
    config = identity['config']
    if config['run']['id'] != args.abandon or pathlib.Path(config['paths']['output']).resolve() != output:
        raise ValueError('abandonment requires the exact admitted run ID and output')
    database = pathlib.Path(config['paths']['database']).resolve(strict=True)
    if not (database.parent/'OWNER').read_text().startswith('akasha-otsuge MVP-003 '):
        raise ValueError('foreign database')
    lock = (database/'.akasha-case-owner.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    import_lock = None
    if (database.parent/'controller.lock').exists():
        import_lock = (database.parent/'controller.lock').open('a')
        fcntl.flock(import_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    if json.loads(state_path.read_text()) != state:
        raise ValueError('run changed before exclusive ownership')
    owner = json.loads((database/'.akasha-generation-owner.json').read_text())
    if owner != dict(output=str(output), run_id=args.abandon):
        raise ValueError('run no longer owns the database')
    stopped(state)
    if state['phase'] == 'completed':
        raise ValueError('cannot abandon a completed generation operation')
    legacy = 'execution_request_sha256' not in identity
    if legacy and identity['controller_sha256'] != 'f485cdc29afd8d60963888dceebab4d27ba0859b134965a97bece7b88dc5a0f3':
        raise ValueError('unknown historical generation identity')
    external = {}
    if 'manifest_sha256' in identity:
        # The initial controller retained the request but referenced an external manifest.
        manifest = pathlib.Path(json.loads((output/'request.json').read_text())['manifest']).resolve(strict=True) if legacy else output/'manifest.json'
        if digest(manifest) != identity['manifest_sha256']:
            raise ValueError('retained manifest identity changed')
        if legacy:
            external[str(manifest)] = identity['manifest_sha256']
    if (tomllib.loads((output/'config.toml').read_text()) != config
            or digest(output/'probe') != identity['binary_sha256']
            or digest(output/'request.json') != identity.get('execution_request_sha256', identity['request_sha256'])):
        raise ValueError('retained generation identity changed')
    for name, expected in state.get('result', {}).get('artifact_sha256', {}).items():
        path = (output/name).resolve(strict=True)
        if not path.is_relative_to(output) or digest(path) != expected:
            raise ValueError('retained operation artifact changed')
    if state['phase'] == 'abandoned':
        for name, expected in state['abandonment']['artifact_sha256'].items():
            if digest(output/name) != expected:
                raise ValueError('abandoned artifact changed')
        print(json.dumps(state['abandonment']))
        return
    if state['phase'] not in {'admitted', 'running', 'interrupted'}:
        raise ValueError('unknown generation phase')
    artifacts = {}
    for path in sorted(output.rglob('*')):
        if path.is_symlink():
            raise ValueError('run contains a symbolic link')
        if path.is_file() and path != state_path:
            artifacts[str(path.relative_to(output))] = digest(path)
    receipt = dict(run_id=args.abandon, status='failed', effects='unknown',
        reason='explicit stopped-run abandonment; successor must reconcile native state',
        prior_state_sha256=digest(state_path), artifact_sha256=artifacts, external_artifact_sha256=external)
    state.update(phase='abandoned', abandonment=receipt)
    publish(state_path, state)
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
