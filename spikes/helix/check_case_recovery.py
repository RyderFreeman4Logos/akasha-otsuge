#!/usr/bin/env python3
"""Kill an exact workload child after durable effects; check explicit reconciliation."""

import json
import os
import pathlib
import signal
import subprocess
import sys
import time
import uuid

from run_case import digest, process_identity, publish


def main():
    binary, database, fixture, output = map(lambda value:pathlib.Path(value).resolve(), sys.argv[1:])
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 owned case recovery evidence\n')
    controller = pathlib.Path(__file__).with_name('run_case.py')
    configs = {}
    for name, phase, seconds in [('completed','steady',60),('interrupted','steady',60),('successor','burst',1)]:
        run_id = 'recovery-' + name + '-' + uuid.uuid4().hex[:16]
        config = output/(name+'.toml')
        config.write_text('[run]\nid = '+json.dumps(run_id)+'\nprofile = "low"\nphase = '+json.dumps(phase)+f'\nseconds = {seconds}\ndiagnostic = true\n[paths]\n'+
            ''.join(key+' = '+json.dumps(str(value))+'\n' for key,value in {'binary':binary,'database':database,'fixture':fixture,'output':output/name,'corpus_manifest':''}.items())+
            '[reliability]\nmax_attempts = 1\n')
        configs[name] = (config,run_id)
    publish(output/'admission.json',{'binary_sha256':digest(binary),'controller_sha256':digest(controller),
        'configs':{name:digest(value[0]) for name,value in configs.items()},'pid':os.getpid(),'process':process_identity(os.getpid())})
    def command(name,*extra):return [sys.executable,str(controller),str(configs[name][0]),*extra]
    with (output/'completed.log').open('x') as log:
        subprocess.run(command('completed'),stdout=log,stderr=subprocess.STDOUT,check=True,timeout=1000)
    prior_digest = digest(output/'completed/state.json')
    publish(output/'completed-checkpoint.json',{'state_sha256':prior_digest})
    interrupted = output/'interrupted'
    child = None
    unit = None
    with (output/'interrupted.log').open('x') as log:
        try:
            child = subprocess.Popen(command('interrupted'),stdout=log,stderr=subprocess.STDOUT)
            deadline = time.monotonic()+90
            while time.monotonic()<deadline:
                if child.poll() is not None:raise ValueError('case exited before crash barrier')
                if (interrupted/'state.json').exists():unit=json.loads((interrupted/'state.json').read_text())['unit']
                progress = interrupted/'runtime/progress.json'
                count = json.loads(progress.read_text())['completed'] if progress.exists() else 0
                if count >= 10:
                    rows = [json.loads(line) for line in (interrupted/'runtime/requests.jsonl').read_text().splitlines()[:count]]
                    commits = [row for row in rows if row['status']=='success' and row['operation']=='write']
                    if commits:break
                time.sleep(.1)
            else:raise TimeoutError('no durable workload effect at crash barrier')
            samples = [json.loads(line) for line in (interrupted/'memory.jsonl').read_text().splitlines()]
            executed = next(row for row in samples if row['event']=='executable')
            live = process_identity(executed['pid'])
            assert live and live['state']!='Z' and live['start_ticks']==executed['start_ticks']
            publish(output/'crash-barrier.json',{'executed':executed,'durable_successful_write':commits[0],
                'checkpoint':json.loads(progress.read_text()),'signal':'SIGKILL'})
            os.kill(executed['pid'],signal.SIGKILL)
            assert child.wait(timeout=60)!=0
        finally:
            if unit:
                active = subprocess.check_output(['systemctl','--user','show',unit,'-p','ActiveState','--value'],text=True).strip()
                if active not in {'inactive','failed'}:
                    subprocess.run(['systemctl','--user','stop',unit],check=False,timeout=60)
            if child is not None and child.poll() is None:
                child.wait(timeout=60)
    before = {name:digest(interrupted/name) for name in ['engine.log','memory.jsonl','runtime/requests.jsonl','runtime/baseline.json']}
    publish(output/'interrupted-checkpoint.json',before)
    ordinary = subprocess.run(command('interrupted','--resume'),capture_output=True,text=True,timeout=30)
    assert ordinary.returncode != 0 and 'explicitly --abandon' in ordinary.stderr
    abandoned = subprocess.run(command('interrupted','--abandon',configs['interrupted'][1]),capture_output=True,text=True,timeout=30)
    state = json.loads((interrupted/'state.json').read_text())
    assert abandoned.returncode == 1 and state['result']['effects']=='unknown'
    assert state['phase']=='abandoned'
    assert before == {name:digest(interrupted/name) for name in before}
    with (output/'successor.log').open('x') as log:
        subprocess.run(command('successor'),stdout=log,stderr=subprocess.STDOUT,check=True,timeout=1000)
    assert digest(output/'completed/state.json') == prior_digest
    reused = subprocess.run(command('completed','--resume'),capture_output=True,text=True,timeout=30)
    assert reused.returncode == 0, reused.stderr
    result = {'status':'passed','checks':['60-second real case','SIGKILL after fsynced successful write',
        'ordinary resume refuses ambiguous effects','exact run-ID abandonment preserves runtime artifacts',
        'later case executes','prior completed receipt remains unchanged and reusable'],
        'binary_sha256':digest(binary),'prior_state_sha256':prior_digest,'interrupted_artifacts':before,
        'not_proven':['power loss','full-size storage recovery','generation activation recovery']}
    publish(output/'result.json',result)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
