#!/usr/bin/env python3
"""Verify native legacy-manifest binding respects write reservations."""
import hashlib
import json
import os
import pathlib
import sys
import tempfile
import time

from check_real_search import run_native

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'helix'))
from run_case import digest, process_identity, publish


def main():
    binary, legacy, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 legacy binding receipts\n')
    records, started, total = [], time.monotonic(), 15
    with tempfile.TemporaryDirectory(prefix='akasha-legacy-binding-') as temporary:
        root = pathlib.Path(temporary)
        (root / 'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        database, source, manifest = [root / name for name in ['database', 'input.jsonl', 'manifest.json']]
        raw = (json.dumps(dict(key='original', body='original astronomy', partition='p0')) + '\n').encode()
        source.write_bytes(raw)
        bundle = dict(model='synthetic-text-only', model_fingerprint='a' * 64,
                      dimension=1536, chunker='whole-record-v1', schema=1)
        publish(manifest, dict(embedding_bundle=bundle, corpus=dict(documents=1, vectors=0,
            prepared_bytes=len(raw), prepared_sha256=hashlib.sha256(raw).hexdigest(),
            indexed_body_utf8_bytes=len('original astronomy'))))
        publish(output / 'admission.json', dict(binary_sha256=digest(binary), legacy_binary_sha256=digest(legacy),
            harness_sha256=digest(pathlib.Path(__file__)), helper_sha256=digest(pathlib.Path(__file__).with_name('check_real_search.py')),
            controller=process_identity(os.getpid()), temporary_root=temporary, planned_commands=total))

        def invoke(executable, *args, error=None, reserve=False):
            previous = os.environ.pop('AKASHA_SPIKE_DISK_RESERVE_BYTES', None)
            try:
                if reserve:
                    os.environ['AKASHA_SPIKE_DISK_RESERVE_BYTES'] = str(2**64 - 1)
                stdout, stderr = run_native([str(executable), str(database), *map(str, args)], output, records)
            finally:
                os.environ.pop('AKASHA_SPIKE_DISK_RESERVE_BYTES', None)
                if previous is not None:
                    os.environ['AKASHA_SPIKE_DISK_RESERVE_BYTES'] = previous
            records[-1].update(expected_error=error, reserve_bytes=str(2**64 - 1) if reserve else None)
            publish(output / 'commands.json', records)
            assert not records[-1]['timed_out'] and records[-1]['exit'] == (1 if error else 0), (len(records)-1, stderr[-2000:])
            if error:
                assert error.encode() in stderr, stderr[-2000:]
            elapsed = time.monotonic() - started
            publish(output / 'progress.json', dict(completed=len(records), total=total,
                elapsed_s=elapsed, rate_per_s=len(records)/elapsed, eta_s=(total-len(records))*elapsed/len(records)))
            return None if error else json.loads(stdout)

        def call(executable, op, generation=1, error=None, reserve=False, **fields):
            request = output / f'{len(records):03d}.request.json'
            publish(request, dict(op=op, generation=generation, **fields))
            return invoke(executable, '--generation', request, error=error, reserve=reserve)

        invoke(legacy, '--load', source, manifest)
        call(legacy, 'begin', bundle=bundle)
        original = call(legacy, 'backfill', waves=1)
        assert original['cursor'] == 1 and original['backfill_deadline_ms'] is not None
        call(binary, 'begin', bundle=bundle, manifest=str(manifest), reserve=True, error='disk reservation reached')
        call(binary, 'status', error='missing persisted corpus manifest')
        call(binary, 'begin', 2, bundle=bundle, manifest=str(manifest), reserve=True, error='disk reservation reached')
        call(binary, 'status', error='missing persisted corpus manifest')
        call(binary, 'status', 2, error='missing generation')
        assert call(binary, 'begin', bundle=bundle, manifest=str(manifest)) == original
        assert call(binary, 'begin', bundle=bundle, reserve=True) == original
        assert call(binary, 'status', reserve=True) == original
        assert call(binary, 'begin', 2, bundle={**bundle, 'schema': 2})['generation'] == 2
        call(binary, 'begin', 3, bundle=bundle, reserve=True, error='disk reservation reached')
        call(binary, 'status', 3, error='missing generation')
        assert invoke(binary, 'verify-import')['prefix'] == hashlib.sha256(raw).hexdigest()
        assert len(records) == total
    publish(output / 'result.json', dict(status='passed', commands=total, seconds=time.monotonic()-started,
        original_state_and_deadlines_preserved=True, rejected_binding_left_metadata_absent=True,
        existing_read_and_reuse_allowed=True, temporary_root_removed=not pathlib.Path(temporary).exists(),
        artifact_sha256={p.name:digest(p) for p in output.iterdir() if p.is_file()}))
    print(json.dumps(dict(status='passed', commands=total, seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
