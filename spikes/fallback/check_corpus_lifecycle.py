#!/usr/bin/env python3
"""Revise actual search candidates, then verify restoration and live permissions."""
import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import struct
import sys
import time

from check_real_search import run_native
from run_case import digest, generation_id, process_identity, publish


def body_hash(body):
    return hashlib.sha256(body.encode()).hexdigest()


def check_entry(entry, memory, seq, scope, body, text_only, source_key):
    assert entry is not None
    assert (entry['memory'], entry['seq'], entry['scope'], entry['active'],
            body_hash(entry['body']), entry['text_only'], entry['source_key']) == (
                memory, seq, scope, True, body_hash(body), text_only, source_key)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ['binary', 'database', 'fixture', 'output']:
        parser.add_argument(key, type=pathlib.Path)
    parser.add_argument('--generation', type=int, required=True)
    parser.add_argument('--exclude-fixture', type=pathlib.Path)
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    if not generation_id(args.generation):
        raise ValueError('generation must be a positive u64')
    for key, value in dict(LANCE_FTS_NUM_SHARDS='1', LANCE_FTS_PARTITION_SIZE='32',
                           MALLOC_MMAP_THRESHOLD_='131072').items():
        if os.environ.get(key) != value:
            raise ValueError(f'child startup requires {key}={value}')
    cgroup_line = next(line for line in pathlib.Path('/proc/self/cgroup').read_text().splitlines()
                       if line.startswith('0::'))
    cgroup = pathlib.Path('/sys/fs/cgroup') / cgroup_line[3:].lstrip('/')
    if ((cgroup / 'memory.max').read_text().strip() != str(640 * 1024**2)
            or (cgroup / 'memory.swap.max').read_text().strip() != '0'):
        raise ValueError('requires a dedicated low-budget observer service')
    binary, database, fixture = [getattr(args, k).resolve(strict=True)
                                for k in ['binary', 'database', 'fixture']]
    output = args.output.resolve()
    if fixture.stat().st_size > 8 * 1024**2:
        raise ValueError('fixture exceeds 8 MiB')
    population = json.loads(fixture.read_text())
    sidecar = population['vector_file']
    assert population['dimension'] == 1536 and pathlib.Path(sidecar['name']).name == sidecar['name']
    vectors = fixture.parent / sidecar['name']
    assert vectors.stat().st_size == sidecar['bytes'] and digest(vectors) == sidecar['sha256']
    selected = {}
    for query in population['queries']:
        if query.get('vector_offset') is not None:
            selected.setdefault(query['partition'], query)
    assert set(selected) == {f'p{i}' for i in range(8)}
    excluded = set()
    if args.exclude_fixture:
        assert args.exclude_fixture.stat().st_size <= 8 * 1024**2
        excluded = {q['memory'] for q in json.loads(args.exclude_fixture.read_text())['queries']}
    identity = dict(binary_sha256=digest(binary), database=str(database), generation=args.generation,
                    fixture_sha256=digest(fixture), vector_sha256=digest(vectors),
                    exclusion_sha256=digest(args.exclude_fixture) if args.exclude_fixture else None,
                    harness_sha256=digest(__file__), native_helper_sha256=digest(pathlib.Path(__file__).with_name('check_real_search.py')),
                    controller_helper_sha256=digest(pathlib.Path(__file__).parents[1] / 'helix/run_case.py'))
    assert (database / 'OWNER').read_text() == 'akasha-otsuge MVP-003 fallback database\n'
    lock = (database / '.akasha-case-owner.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    owner_path = database / '.akasha-case-owner.json'
    for owner_file in [owner_path, database / '.akasha-generation-owner.json']:
        if owner_file.exists():
            prior = json.loads(owner_file.read_text())
            if prior['output'] != str(output):
                assert json.loads((pathlib.Path(prior['output']) / 'state.json').read_text())['phase'] in {'completed', 'abandoned'}
    state_path = output / 'state.json'
    if output.exists():
        if not args.resume:
            raise ValueError('existing output requires explicit --resume')
        state = json.loads(state_path.read_text())
        assert state['identity'] == identity, 'immutable identity changed'
        old = process_identity(state['controller']['pid'])
        assert old is None or old['start_ticks'] != state['controller']['start_ticks']
        old_group = pathlib.Path(state['cgroup']) / 'cgroup.procs'
        assert not old_group.exists() or not old_group.read_text().strip(), 'prior service is still live'
        if state['phase'] == 'completed':
            for name, expected in state['result']['artifact_sha256'].items():
                assert digest(output / name) == expected, name
            print(json.dumps(state['result']))
            return
        records = json.loads((output / 'commands.json').read_text()) if (output / 'commands.json').exists() else []
        for record in records:
            old = process_identity(record['child']['pid'])
            assert old is None or old['start_ticks'] != record['child']['start_ticks']
        steps = json.loads((output / 'steps.json').read_text()) if (output / 'steps.json').exists() else {}
    else:
        if args.resume:
            raise ValueError('cannot resume absent output')
        output.mkdir(mode=0o700)
        (output / 'OWNER').write_text('akasha-otsuge MVP-003 corpus lifecycle receipts\n')
        state, records, steps = dict(identity=identity, phase='admitted', started_unix_s=time.time()), [], {}
    state.update(controller=process_identity(os.getpid()), cgroup=str(cgroup), phase='running')
    publish(state_path, state)
    publish(owner_path, dict(output=str(output), run_id=output.name))
    started, checked, cases = time.monotonic(), 0, []
    principal = 'corpus-life-' + hashlib.sha256(str(output).encode()).hexdigest()[:16]

    def invoke(label, channel, request):
        request_path = output / (label + '.request.json')
        command_args = [channel, str(request)] if channel == 'corpus-get' else [channel, str(request_path)]
        encoded = json.dumps(request, sort_keys=True).encode()
        fingerprint = hashlib.sha256(channel.encode() + b'\0' + encoded).hexdigest()
        reply_path = output / (label + '.reply.json')
        if label in steps:
            assert steps[label]['request_sha256'] == fingerprint
            assert digest(reply_path) == steps[label]['reply_sha256']
            return json.loads(reply_path.read_text())
        if channel != 'corpus-get':
            publish(request_path, request)
        stdout, stderr = run_native([str(binary), str(database), *command_args], output, records)
        assert records[-1]['exit'] == 0 and not records[-1]['timed_out'], (label, stderr[-2000:])
        result = json.loads(stdout)
        publish(reply_path, result)
        steps[label] = dict(request_sha256=fingerprint, reply_sha256=digest(reply_path))
        publish(output / 'steps.json', steps)
        elapsed = time.monotonic() - started
        publish(output / 'progress.json', dict(completed=checked, total=16, native_completed=len(steps),
                elapsed_s=elapsed, rate_per_s=checked / elapsed, eta_s=(16-checked)*elapsed/checked if checked else None,
                active=label, controller=process_identity(os.getpid())))
        return result

    def authority(label, **request):
        return invoke(label, '--authority', request)

    def generation(label, **request):
        return invoke(label, '--generation', dict(generation=args.generation, **request))

    try:
        base = generation('generation', op='status')
        assert base['ready'] and base['cursor'] == base['documents']
        status = generation('active', op='lifecycle_status')
        assert status['active'] == args.generation
        bundle = base['bundle']
        for scope in range(8):
            authority(f'permission-{scope}', op='permission', principal=principal, scope=scope, allowed=True)
        with vectors.open('rb') as vector_file:
            for scope in range(8):
                query = selected[f'p{scope}']
                offset = query['vector_offset']
                assert type(offset) is int and offset >= 0 and offset % (1536 * 4) == 0
                vector_file.seek(offset)
                query_vector = list(struct.unpack('<1536f', vector_file.read(1536 * 4)))
                for mode, clause in [('bm25', dict(term=query['term'])), ('ann', dict(vector=query_vector))]:
                    tag = f'p{scope}-{mode}'
                    def search(label, **updates):
                        return generation(tag+'-'+label, **dict(op='query_current', bundle=bundle,
                            principal=principal, scope=scope, **clause, **updates))
                    before = search('before')
                    target = next(hit for hit in before['hits'] if hit['memory'] not in excluded
                                  and 1_000_000 <= hit['memory'] < 1_000_000 + base['documents'])
                    memory = target['memory']
                    original = invoke(tag+'-source', 'corpus-get', memory)
                    head = authority(tag+'-head', op='get', memory=memory, at=before['validated_at_seq'])
                    assert original['partition'] == f'p{scope}' and head['body'] == original['body']
                    assert head['active'] and head['scope'] == scope and len(head['body'].encode()) <= 128*1024-128
                    assert head['source_key'] == original['key']
                    previous, commits = head['seq'], []
                    corrected = head['body'] + '\nCorpus lifecycle correction.'
                    for stage, active, changed_scope, body, text_only in [
                        ('correct', True, scope, corrected, True),
                        ('rescope', True, (scope+1)%8, corrected, True),
                        ('retract', False, (scope+1)%8, corrected, True),
                        ('restore', True, scope, head['body'], head['text_only'])]:
                        request_id = principal+'-'+tag+'-'+stage
                        changed = authority(tag+'-'+stage, op='apply', change=dict(request_id=request_id,
                            memory=memory, expected=previous, scope=changed_scope, active=active,
                            body=body, text_only=text_only))
                        seq = changed['commit_seq']
                        assert seq > previous
                        committed = generation(tag+'-'+stage+'-committed', op='indexed_receipt', request_id=request_id)
                        assert committed['commit_seq'] == seq and committed['stage'] == 'committed'
                        stale = authority(tag+'-'+stage+'-stale', op='revalidate', principal=principal,
                            scope=scope, read_seq=before['read_seq'], min_commit_seq=seq, hits=before['hits'])
                        assert all(h['memory'] != memory for h in stale['hits'])
                        unseen = search(stage+'-unindexed')
                        assert all(h['memory'] != memory for h in unseen['hits'])
                        current = authority(tag+'-'+stage+'-current', op='current', principal=principal, memory=memory)
                        if active:
                            check_entry(current['entry'], memory, seq, changed_scope, body, text_only, original['key'])
                        else:
                            assert current['entry'] is None
                        generation(tag+'-'+stage+'-text', op='apply', family='text', seq=seq)
                        vector = None
                        if active and not text_only:
                            vector = dict(model=bundle['model'], model_fingerprint=bundle['model_fingerprint'],
                                          body_sha256=body_hash(body), values=original['embedding'])
                        generation(tag+'-'+stage+'-vector', op='apply', family='vector', seq=seq, vector=vector)
                        indexed = generation(tag+'-'+stage+'-indexed', op='indexed_receipt', request_id=request_id)
                        assert indexed['stage'] == 'indexed' and indexed['commit_seq'] == seq
                        assert indexed['watermarks'] == dict(text=seq, vector=seq)
                        visible = search(stage+'-indexed-query', min_commit_seq=seq)
                        found = [h for h in visible['hits'] if h['memory'] == memory]
                        should_find = stage == 'restore' or (stage == 'correct' and mode == 'bm25')
                        assert bool(found) == should_find, (tag, stage, visible)
                        if found: assert found[0]['seq'] == seq
                        if stage == 'rescope' and mode == 'bm25':
                            moved = generation(tag+'-new-scope', op='query_current', bundle=bundle,
                                principal=principal, scope=changed_scope, term=query['term'], min_commit_seq=seq)
                            assert any(h['memory'] == memory and h['seq'] == seq for h in moved['hits'])
                        previous = seq
                        commits.append(seq)
                    fixed = authority(tag+'-fixed-history', op='get', memory=memory, at=before['validated_at_seq'])
                    assert fixed == head
                    denied = authority(tag+'-revoke', op='permission', principal=principal, scope=scope, allowed=False)
                    assert search('revoked')['hits'] == []
                    assert authority(tag+'-denied-current', op='current', principal=principal, memory=memory)['entry'] is None
                    assert authority(tag+'-denied-history', op='history', principal=principal, scope=scope,
                                     at=before['validated_at_seq'], cursor=memory-1, limit=1)['entries'] == []
                    allowed = authority(tag+'-permit', op='permission', principal=principal, scope=scope, allowed=True)
                    assert allowed['revision'] == denied['revision']+1
                    assert any(h['memory'] == memory and h['seq'] == previous for h in search('permitted')['hits'])
                    cases.append(dict(scope=scope, mode=mode, memory=memory, original_seq=head['seq'], commits=commits))
                    checked += 1
                    publish(output / 'cases.json', cases)
                    elapsed = time.monotonic() - started
                    publish(output / 'progress.json', dict(completed=checked, total=16, native_completed=len(steps),
                            elapsed_s=elapsed, rate_per_s=checked/elapsed, eta_s=(16-checked)*elapsed/checked,
                            active=tag, controller=process_identity(os.getpid())))
        final = generation('final', op='lifecycle_status')
        assert final['active'] == args.generation and final['text']['watermark'] == final['vector']['watermark'] == final['committed']
        publish(output / 'progress.json', dict(phase='completed', completed=16, total=16, native_completed=len(steps),
                elapsed_s=time.monotonic()-started, eta_s=0, controller=process_identity(os.getpid())))
        result = dict(status='passed', cases=cases, native_commands=len(records), unique_steps=len(steps),
                      seconds=time.monotonic()-started, generation=args.generation, final=final,
                      not_proven=['simultaneous query/build scheduling', 'sustained workload'],
                      artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file() and p.name != 'state.json'})
        state.update(phase='completed', result=result)
        publish(state_path, state)
        print(json.dumps(dict(status='passed', cases=checked, native_commands=len(records))))
    except BaseException as error:
        state.update(phase='interrupted', error=str(error) or type(error).__name__)
        publish(state_path, state)
        raise


if __name__ == '__main__':
    main()
