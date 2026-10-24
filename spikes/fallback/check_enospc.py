#!/usr/bin/env python3
"""Force ENOSPC after staging a native transaction in an isolated 8MiB mount."""
import errno
import json
import os
import pathlib
import selectors
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'helix'))
from run_case import digest, process_identity, publish


def inside(binary, output):
    records = []
    with tempfile.TemporaryDirectory(prefix='akasha-fallback-enospc-') as temporary:
        root = pathlib.Path(temporary)
        publish(output/'mount-admission.json',dict(temporary_root=temporary,controller=process_identity(os.getpid()),namespace=os.readlink('/proc/self/ns/mnt')))
        subprocess.run(['mount', '-t', 'tmpfs', '-o', 'size=8M,mode=0700', 'tmpfs', str(root)], check=True)
        child = None
        try:
            (root / 'OWNER').write_text('akasha-otsuge MVP-003 fallback database\n')
            def invoke(operation):
                result = subprocess.run([str(binary), str(root), operation], capture_output=True,
                    timeout=60, env=dict(os.environ, AKASHA_SPIKE_PROFILE='low'))
                name=f'{len(records):02d}-{operation}'
                (output / (name+'.stdout')).write_bytes(result.stdout)
                (output / (name+'.stderr')).write_bytes(result.stderr)
                records.append(dict(operation=operation,exit=result.returncode))
                assert result.returncode == 0, result.stderr
                return json.loads(result.stdout)
            assert invoke('history-write') == dict(versions=2)
            with (output/'disk-full.stdout').open('xb') as stdout:
                child=subprocess.Popen([str(binary),str(root),'disk-full'],stdin=subprocess.PIPE,
                    stdout=stdout,stderr=subprocess.PIPE,env=dict(os.environ,AKASHA_SPIKE_PROFILE='low'))
                identity=process_identity(child.pid);prefix=b''
                with selectors.DefaultSelector() as selector:
                    selector.register(child.stderr,selectors.EVENT_READ)
                    until=time.monotonic()+60
                    while time.monotonic()<until:
                        assert child.poll() is None, prefix
                        if selector.select(timeout=1):
                            prefix+=os.read(child.stderr.fileno(),4096)
                            if b'PROBE_CHECKPOINT:disk-full\n' in prefix:break
                    else:raise TimeoutError('staged transaction barrier absent')
                assert process_identity(child.pid)['start_ticks']==identity['start_ticks']
                filler=root/'owned-filler';written=0
                fd=os.open(filler,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
                try:
                    while True:
                        try:written+=os.write(fd,b'x'*65536)
                        except OSError as error:
                            assert error.errno==errno.ENOSPC
                            break
                finally:os.close(fd)
                assert shutil.disk_usage(root).free==0
                child.stdin.write(b'go\n');child.stdin.flush();child.stdin.close();child.stdin=None
                _,tail=child.communicate(timeout=60)
                (output/'disk-full.stderr').write_bytes(prefix+tail)
                assert child.returncode==1 and b'COMMIT_ERROR_PHASE' in tail, tail
                assert b'No space left on device' in tail, tail
                assert process_identity(child.pid) is None
                assert not (output/'disk-full.stdout').read_bytes(), 'failed commit was acknowledged'
                filler.unlink()
                assert invoke('read')['values']==[None]*5
                assert invoke('history-read')==dict(current='two',at_one='one',at_two='two')
                assert invoke('write')==dict(committed=True,reused=False)
                assert invoke('read')['values']==['one']*5
                publish(output/'inside-result.json',dict(status='passed',filler_bytes=written,
                    kernel_errno=errno.ENOSPC,mount_namespace=os.readlink('/proc/self/ns/mnt'),
                    native_child=identity,failed_commit_exit=child.returncode,
                    prior_history_preserved=True,failed_keys_absent=True,retry_committed=True,
                    records=records,scope='isolated small tmpfs transaction; no SSD power-loss claim'))
        finally:
            if child is not None and child.poll() is None:
                child.kill();child.communicate(timeout=10)
            # Retain the small authority fixture before disposing the private mount.
            try:
                if (root/'authority').exists():shutil.copytree(root/'authority',output/'authority-snapshot')
            finally:
                subprocess.run(['umount',str(root)],check=True,timeout=15)
    publish(output/'cleanup.json',dict(temporary_root=temporary,removed=not root.exists()))


def main():
    binary,output=pathlib.Path(sys.argv[1]).resolve(strict=True),pathlib.Path(sys.argv[2]).resolve()
    if len(sys.argv)==4 and sys.argv[3]=='--inside':
        inside(binary,output);return
    output.mkdir(mode=0o700);(output/'OWNER').write_text('akasha-otsuge MVP-003 ENOSPC receipts\n')
    publish(output/'admission.json',dict(binary_sha256=digest(binary),harness_sha256=digest(pathlib.Path(__file__)),
        controller=process_identity(os.getpid()),capacity_bytes=8*1024*1024))
    command=['unshare','--user','--map-root-user','--mount','--propagation','private',sys.executable,
             str(pathlib.Path(__file__).resolve()),str(binary),str(output),'--inside']
    with (output/'namespace.log').open('xb') as log:
        result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=420)
    assert result.returncode==0, (result.returncode,str(output/'namespace.log'))
    result=json.loads((output/'inside-result.json').read_text())
    assert json.loads((output/'cleanup.json').read_text())['removed']
    result['artifact_sha256']={str(p.relative_to(output)):digest(p) for p in output.rglob('*') if p.is_file()}
    publish(output/'result.json',result);print(json.dumps(dict(status='passed',kernel_errno=errno.ENOSPC)))


if __name__=='__main__':main()
