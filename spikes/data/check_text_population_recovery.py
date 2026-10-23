#!/usr/bin/env python3
"""Kill real source replay after a durable selection, then verify exact resume."""
import json
import os
import pathlib
import signal
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]/'helix'))
from run_case import digest, process_identity, publish


def main():
    inputs, manifest, reference, output = [pathlib.Path(p).resolve(strict=True) for p in sys.argv[1:4]] + [pathlib.Path(sys.argv[4]).resolve()]
    script = pathlib.Path(__file__).with_name('prepare_text_population.py').resolve()
    observer = script.parents[1]/'measure.py'
    output.mkdir(mode=0o700)
    (output/'OWNER').write_text('akasha-otsuge MVP-003 text preparation recovery evidence\n')
    prepared = output/'preparation'
    started, attempts = time.monotonic(), []
    identity = dict(controller=process_identity(os.getpid()), harness_sha256=digest(pathlib.Path(__file__)),
        script_sha256=digest(script), observer_sha256=digest(observer), interpreter=str(pathlib.Path(sys.executable).resolve()),
        manifest_sha256=digest(manifest), reference_sha256=digest(reference))
    publish(output/'admission.json', identity)

    def launch(name, resume, interrupt=False):
        unit = 'akasha-text-recovery-'+uuid.uuid4().hex
        command = ['systemd-run','--user','--quiet','--wait','--pipe','--collect','--unit='+unit,
            '-p','MemoryMax=8589934592','-p','MemorySwapMax=0','-p','RuntimeMaxSec=3600',
            sys.executable,str(observer),str(output/(name+'-memory.jsonl')),sys.executable,str(script),
            str(inputs),str(manifest),str(prepared),*(['--resume'] if resume else [])]
        record = dict(name=name,unit=unit,command=command,phase='admitted')
        attempts.append(record)
        publish(output/'state.json',dict(identity=identity,phase=name,attempts=attempts))
        with (output/(name+'.log')).open('x') as log:
            child = subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic()+3600
                while child.poll() is None:
                    if time.monotonic() >= deadline:
                        raise TimeoutError('source replay exceeded admitted unit deadline')
                    state_path = prepared/'state.json'
                    state = json.loads(state_path.read_text()) if state_path.exists() else None
                    progress_path = prepared/'progress.json'
                    native = json.loads(progress_path.read_text()) if progress_path.exists() else None
                    publish(output/'progress.json',dict(phase=name,completed=len(attempts)-1,total=2,
                        elapsed_s=time.monotonic()-started,rate_per_s=None,eta_s=None,native=native,
                        controller=process_identity(os.getpid()),unit=unit,config_epoch=1))
                    if interrupt and state and len(state['selected']) >= 16:
                        actual = process_identity(state['pid'])
                        assert actual and actual['start_ticks'] == state['start_ticks'] and actual['state'] != 'Z'
                        publish(output/'before-kill-state.json',state)
                        record['killed_process'] = actual
                        os.kill(actual['pid'],signal.SIGKILL)
                        break
                    time.sleep(1)
                code = child.wait(timeout=60)
                assert (code != 0 and 'killed_process' in record) if interrupt else code == 0, (name,code)
            finally:
                if child.poll() is None:
                    subprocess.run(['systemctl','--user','stop',unit],check=True,timeout=60)
                    child.wait(timeout=60)
        active = subprocess.check_output(['systemctl','--user','show',unit,'-p','ActiveState','--value'],text=True).strip()
        assert active in {'inactive','failed'}
        events=[json.loads(line) for line in (output/(name+'-memory.jsonl')).read_text().splitlines()]
        assert events[0]['limits']['memory.max']=='8589934592' and events[0]['limits']['memory.swap.max']=='0'
        exits=[row for row in events if row['event']=='exit']
        assert len(exits)==1 and (exits[0]['code']==-9 if interrupt else exits[0]['code']==0)
        record.update(phase='interrupted_by_test' if interrupt else 'completed',exit=code,native_exit=exits[0])
        publish(output/'state.json',dict(identity=identity,phase=name,attempts=attempts))

    launch('interrupted',False,True)
    stopped=json.loads((prepared/'state.json').read_text())
    assert stopped['phase'] != 'completed' and 16 <= len(stopped['selected']) < 512
    assert process_identity(stopped['pid']) is None
    publish(output/'after-kill-state.json',stopped)
    launch('resumed',True)
    final=json.loads((prepared/'state.json').read_text())
    assert final['phase']=='completed' and final['selected'][:len(stopped['selected'])]==stopped['selected']
    assert digest(prepared/'text-queries.json')==identity['reference_sha256']
    before={str(p.relative_to(prepared)):digest(p) for p in prepared.rglob('*') if p.is_file()}
    with (output/'completed-reuse.log').open('x') as log:
        subprocess.run([sys.executable,str(script),str(inputs),str(manifest),str(prepared),'--resume'],stdout=log,stderr=subprocess.STDOUT,check=True,timeout=30)
    assert before=={str(p.relative_to(prepared)):digest(p) for p in prepared.rglob('*') if p.is_file()}
    elapsed=time.monotonic()-started
    publish(output/'state.json',dict(identity=identity,phase='completed',attempts=attempts))
    publish(output/'progress.json',dict(phase='completed',completed=2,total=2,elapsed_s=elapsed,eta_s=0))
    result=dict(status='passed',seconds=elapsed,interrupted_selected=len(stopped['selected']),
        final_selected=len(final['selected']),query_sha256=identity['reference_sha256'],attempts=attempts,
        artifact_sha256={str(p.relative_to(output)):digest(p) for p in output.rglob('*') if p.is_file()},
        not_tested=['physical power loss','engine memory acceptance','file-cache-cold search'])
    publish(output/'result.json',result)
    print(json.dumps({k:result[k] for k in ['status','seconds','interrupted_selected','final_selected','query_sha256']}))


if __name__ == '__main__':
    main()
