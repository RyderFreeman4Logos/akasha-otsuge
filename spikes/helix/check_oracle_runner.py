#!/usr/bin/env python3
"""Verify owned text-history seeding, exact replay, validation and preflight refusal."""
import json
import pathlib
import subprocess
import sys
import tempfile
import time

from run_case import digest, publish


def main():
    binary, source, output = [pathlib.Path(p).resolve() for p in sys.argv[1:4]]
    backend = sys.argv[4] if len(sys.argv) == 5 else 'helix'
    assert backend in {'helix', 'fallback'} and len(sys.argv) in {4, 5}
    runner = pathlib.Path(__file__).with_name('run_generation.py').resolve()
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 owned oracle runner evidence\n')
    records, started = [], time.monotonic()
    with tempfile.TemporaryDirectory(prefix='akasha-oracle-runner-') as temporary:
        root=pathlib.Path(temporary);database=root/'database'
        (root/'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        publish(output/'admission.json',dict(binary_sha256=digest(binary),runner_sha256=digest(runner),harness_sha256=digest(__file__),temporary_root=temporary))
        def run(name,command,success=True):
            result=subprocess.run(command,capture_output=True,text=True,timeout=300)
            (output/(name+'.log')).write_text(result.stdout+result.stderr)
            records.append(dict(name=name,command=command,exit=result.returncode))
            publish(output/'progress.json',dict(phase=name,completed=len(records),total=10,elapsed_s=time.monotonic()-started))
            assert (result.returncode==0)==success,(name,result.stderr)
            return result.stdout
        def config(name,op,count):
            request=output/(name+'.json');publish(request,dict(op=op,generation=1,count=count))
            path=output/(name+'.toml')
            path.write_text(f'''[run]
id = "{name}"
profile = "low"
seconds = 300
[paths]
binary = {json.dumps(str(binary))}
database = {json.dumps(str(database))}
request = {json.dumps(str(request))}
output = {json.dumps(str(output/name))}
[disk]
start_bytes = 81212254720
reserve_bytes = 36212254720
[reliability]
max_attempts = 1
''')
            return path
        run('load',[str(binary),str(database),'--load',str(source/'input.jsonl'),str(source/'manifest.json')])
        for i,count in enumerate([0,1_000_001,True,'2048']):
            name='invalid-'+str(i)
            run(name,[sys.executable,str(runner),str(config(name,'seed_text',count))],False)
            assert not (output/name).exists()
        seed=config('seed','seed_text',2048)
        run('seed',[sys.executable,str(runner),str(seed)])
        checkpoint=json.loads((database/'oracle-seed-text-2048.json').read_text())
        state=json.loads((output/'seed/state.json').read_text())
        events=[json.loads(line) for line in pathlib.Path(state['attempts'][0]['log']).with_name('memory.jsonl').read_text().splitlines()]
        execution=next(row for row in events if row['event']=='executable')
        assert checkpoint['completed']==checkpoint['total']==2048
        assert checkpoint['pid']==execution['pid'] and checkpoint['start_ticks']==execution['start_ticks']
        publish(output/'seed-checkpoint.json',checkpoint)
        before=digest(output/'seed/state.json')
        run('completed-reuse',[sys.executable,str(runner),str(seed),'--resume'])
        assert digest(output/'seed/state.json')==before
        run('native-replay',[sys.executable,str(runner),str(config('native-replay','seed_text',2048))])
        run('verify',[sys.executable,str(runner),str(config('verify','verify_text',2048))])
        checkpoint=json.loads((database/'oracle-verify-text-2048.json').read_text())
        assert checkpoint['completed']==checkpoint['total'] and checkpoint['completed']>4000
        publish(output/'verify-checkpoint.json',checkpoint)
        if backend == 'fallback':
            status = output/'status.json';publish(status,dict(op='status'))
            count=run('count',[str(binary),str(database),'--authority',str(status)])
            assert json.loads(count)['commit_seq']==2048
        else:
            count=run('count',[str(binary),str(database),"MATCH (c:Counter {key:'global'}) RETURN c.seq"])
            assert json.loads(count)['rows']==[[2048]]
        assert len(records)==10
    publish(output/'result.json',dict(status='passed',records=records,seconds=time.monotonic()-started,
        exact_commits=2048,temporary_database_removed=True,not_tested=['full database history performance','power loss']))
    print(json.dumps(dict(status='passed',checks=len(records),seconds=time.monotonic()-started)))


if __name__ == '__main__':
    main()
