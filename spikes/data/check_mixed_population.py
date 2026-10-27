#!/usr/bin/env python3
"""Check merged real metadata and native admission without opening corpus storage."""
import copy
import json
import os
import pathlib
import subprocess
import sys
import tempfile

from merge_query_populations import combine, digest


def main():
    vectors, text, fixture, old, candidate, output = [pathlib.Path(p).resolve() for p in sys.argv[1:]]
    v, t = json.loads(vectors.read_text()), json.loads(text.read_text())
    expected = combine(v, t)
    assert json.loads(fixture.read_text()) == expected
    output.mkdir(mode=0o700)
    checks = []
    for case in ['manifest', 'overlap', 'vector_for_text', 'vector_offset', 'missing_partition']:
        a, b = copy.deepcopy(v), copy.deepcopy(t)
        if case == 'manifest':b['corpus_manifest_sha256'] = '0'*64
        if case == 'overlap':b['queries'][0]['memory'] = a['queries'][0]['memory']
        if case == 'vector_for_text':b['queries'][0]['embedding'] = [1.0]
        if case == 'vector_offset':a['queries'][1]['vector_offset'] = 0
        if case == 'missing_partition':
            for q in b['queries']:q['partition'] = 'p0'
        try:combine(a, b)
        except ValueError:checks.append(case)
        else:raise AssertionError(case)
    with tempfile.TemporaryDirectory(prefix='akasha-mixed-admission-') as temporary:
        root = pathlib.Path(temporary)
        blocked = root/'must-not-open-database'
        native = []
        for name, binary, admitted in [('old', old, False), ('candidate', candidate, True)]:
            run_output, request = root/name, root/(name+'.json')
            request.write_text(json.dumps(dict(run_id='mixed-admission', phase='burst', seconds=1, diagnostic=True,
                fixture=str(fixture), output=str(run_output), corpus_manifest=None)))
            result = subprocess.run([str(binary), str(blocked), '--workload', str(request)],
                env={**os.environ, 'AKASHA_SPIKE_PROFILE':'invalid-admission-control'}, capture_output=True, text=True, timeout=30)
            (output/(name+'.log')).write_text(result.stdout+result.stderr)
            assert result.returncode != 0 and (run_output/'admission.json').exists() == admitted and not blocked.exists()
            if admitted:
                receipt = json.loads((run_output/'admission.json').read_text())
                assert receipt['fixture_sha256'] == digest(fixture)
                (output/'admission.json').write_text(json.dumps(receipt, indent=2)+'\n')
            native.append(dict(binary_sha256=digest(binary), exit=result.returncode, query_admitted=admitted, storage_opened=False))
    result = dict(status='passed_admission_only', queries=len(expected['queries']), negative_controls=checks,
                  native=native, fixture_sha256=digest(fixture), script_sha256=digest(pathlib.Path(__file__)))
    (output/'result.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
