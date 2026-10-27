#!/usr/bin/env python3
"""Verify native index observation configuration before database effects."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

from run_case import digest, publish


def main():
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    output = pathlib.Path(sys.argv[2]).resolve()
    output.mkdir(mode=0o700)
    records = []
    with tempfile.TemporaryDirectory(prefix='akasha-index-budget-') as temporary:
        database = pathlib.Path(temporary)
        request = output/'status.json'
        publish(request, dict(op='status'))
        publish(output/'admission.json', dict(binary_sha256=digest(binary), harness_sha256=digest(__file__), temporary_root=temporary))
        for name, value in [('zero','0'), ('too-large','604801'), ('negative','-1'), ('invalid','unbounded'), ('explicit','172800'), ('default',None)]:
            environment = dict(os.environ)
            environment.pop('AKASHA_SPIKE_INDEX_OBSERVE_SECONDS', None)
            if value is not None:
                environment['AKASHA_SPIKE_INDEX_OBSERVE_SECONDS'] = value
            with (output/(name+'.log')).open('x') as log:
                result = subprocess.run([str(binary), str(database), '--generation', str(request)], env=environment, stdout=log, stderr=subprocess.STDOUT, timeout=120)
            records.append(dict(name=name, value=value, exit=result.returncode))
            publish(output/'progress.json', dict(completed=len(records), total=6, phase=name))
            text = (output/(name+'.log')).read_text()
            if name in {'explicit','default'}:
                assert result.returncode == 0, text[-2000:]
                assert 'INDEX_OBSERVATION_SECONDS:'+(value or '7200') in text
            else:
                assert result.returncode != 0, (name, text[-2000:])
                assert list(database.iterdir()) == [], 'invalid budget caused database effects'
    publish(output/'result.json', dict(status='passed', records=records, not_tested=['large-index completion time']))
    print(json.dumps(dict(status='passed', checks=6)))


if __name__ == '__main__':
    main()
