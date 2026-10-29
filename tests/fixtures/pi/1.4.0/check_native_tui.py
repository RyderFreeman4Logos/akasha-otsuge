"""Actual Pi CLI and daemon against an explicitly synthetic local model fixture."""
import datetime
import sys
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import threading
import time

E = Path(__file__).parent
A = Path(os.environ['AKASHA_BINARY']).resolve(strict=True)
PI = Path(os.environ['AKASHA_PI_BINARY']).resolve(strict=True)
TEMP_ROOT = Path(os.environ['TMPDIR']).resolve(strict=True)
OUTPUT = Path(sys.argv[1]).resolve()
OUTPUT.mkdir(parents=True, exist_ok=False)
assert shutil.disk_usage(TEMP_ROOT).free >= 128 * 1024 * 1024
spec = importlib.util.spec_from_file_location('fixture_provider', E / 'native_provider.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
os.umask(0o077)
root = Path(tempfile.mkdtemp(prefix='a15-caller-', dir=TEMP_ROOT))
profile, workspace = root / 'agent', root / 'workspace'
profile.mkdir(mode=0o700)
workspace.mkdir(mode=0o700)
provider = module.Provider()
provider.case = 'caller'
env = dict(os.environ, PATH=os.environ['AKASHA_NATIVE_PATH'], PI_CODING_AGENT_DIR=str(profile),
           TMPDIR=str(root), TMP=str(root), TEMP=str(root), PYTHONDONTWRITEBYTECODE='1')
sock = root / 'run/service.sock'
config = root / 'akasha.toml'
config.write_text('[paths]\ndata_dir=' + json.dumps(str(root / 'data')) + '\nsocket_path=' + json.dumps(str(sock))
                  + '\n[access]\nread_roots=[' + json.dumps(str(root)) + ']\n')
base = [str(A), '--config', str(config), '--socket', str(sock)]
receipt = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'scratch': str(root),
           'provider_api': provider.api,
           'kind': 'actual controlling-PTY Pi interactive lifecycle with controlled local SSE; no authenticated model',
           'akasha_sha256': hashlib.sha256(A.read_bytes()).hexdigest(), 'status': 'failed', 'checks': []}
children = []
daemon = None
installed = False


def cli(*args):
    result = subprocess.run([*base, *args], cwd=root, env=env, capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr[:1000]
    return json.loads(result.stdout)['data']



import fcntl, pty, struct, termios
launcher=root/'tty-exec.py'
launcher.write_text("import os,sys,fcntl,termios\nos.setsid()\nfcntl.ioctl(0,termios.TIOCSCTTY,0)\nos.execvpe(sys.argv[1],sys.argv[1:],os.environ)\n")
receipt['tty'] = []
tools=root/'tools';tools.mkdir();
for name in ['fd', 'rg']:
    executable = Path(os.environ['AKASHA_TUI_TOOLS']) / name
    assert executable.is_file() and os.access(executable, os.X_OK)
    (tools / name).symlink_to(executable.resolve(strict=True))
env['PATH']=str(tools)+':'+env['PATH']
for key in ['TMUX','STY','KITTY_WINDOW_ID']: env.pop(key,None)
ready_extension=root/'ready.ts';ready_extension.write_text("import fs from 'node:fs'; export default function(pi) { pi.on('session_start', () => { fs.writeFileSync(process.env.AKASHA_TUI_READY, 'ready'); }); }\n")
class Native:
    def __init__(self,name,session):
        self.screen=bytearray();self.errors=[]
        master,slave=pty.openpty();self.master=master
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',40,120,0,0))
        self.stderr=(OUTPUT/(name+'.terminal.log')).open('xb')
        args=[sys.executable,str(launcher),str(PI),'--session',str(session),'--no-builtin-tools','--no-extensions','--no-mcp','--no-skills','--no-prompt-templates','--no-themes','--no-context-files','--no-approve','-e',str(profile/'extensions/akasha-otsuge/index.ts'),'-e',str(E/'lifecycle.ts'),'-e',str(ready_extension),'--tools','akasha_consult']
        self.process=subprocess.Popen(args,cwd=workspace,env=dict(env,TERM='xterm-256color',AKASHA_TUI_READY=str(root/(name+'.ready'))),stdin=slave,stdout=slave,stderr=slave);os.close(slave);children.append(self)
        def read():
            try:
                while True:
                    try:chunk=os.read(master,65536)
                    except OSError:break
                    if not chunk:break
                    self.screen.extend(chunk);self.stderr.write(chunk);self.stderr.flush()
                    if len(self.screen)>1048576:raise AssertionError('terminal output limit')
            except Exception as e:self.errors.append(repr(e))
        self.thread=threading.Thread(target=read,daemon=True);self.thread.start()
        deadline=time.monotonic()+20
        while not (root/(name+'.ready')).exists():
            assert self.process.poll() is None and time.monotonic()<deadline, 'native session_start readiness missing'
            time.sleep(.02)
        stat=Path(f'/proc/{self.process.pid}/stat').read_text().rsplit(') ',1)[1].split()
        assert int(stat[2])==int(stat[3])==self.process.pid and int(stat[4])!=0
        assert os.tcgetpgrp(master)==self.process.pid
        receipt['tty'].append(dict(name=name,pid=self.process.pid,start_ticks=stat[19],pgid=int(stat[2]),sid=int(stat[3]),tty_nr=int(stat[4]),foreground=True))
    def send(self,message):os.write(self.master,(message+'\r').encode())
    def close(self):
        self.send('/quit');assert self.process.wait(timeout=15)==0
        self.thread.join(timeout=5);assert not self.thread.is_alive() and not self.errors
        os.close(self.master)

def wait_for(check):
    deadline=time.monotonic()+30
    while not check():
        assert all(n.process.poll() is None for n in children if n.process.returncode is None)
        assert not provider.errors,provider.errors
        assert time.monotonic()<deadline,'native progress deadline'
        time.sleep(.05)
def entries(file):
    if not file.exists():return []
    return [json.loads(line) for line in file.read_text().splitlines()]
def final(file):
    return any(r.get('type')=='context_edit' for r in entries(file)) and 'AFTER_TRANSFORM' in file.read_text()
try:
    receipt['other_cli_absent']={n:shutil.which(n,path=env['PATH']) is None for n in ['claude','codex','hermes']};assert all(receipt['other_cli_absent'].values())
    provider.configure(profile)
    daemon_log=(root/'daemon.log').open('wb');daemon=subprocess.Popen([*base,'daemon','run'],cwd=root,env=env,stdin=subprocess.DEVNULL,stdout=daemon_log,stderr=daemon_log,start_new_session=True)
    wait_for(sock.exists);cli('adapter','install','pi','--binary',str(PI),'--agent-dir',str(profile));installed=True
    a_file,b_file=root/'a.jsonl',root/'b.jsonl'
    a=Native('a',a_file);b=Native('b',b_file)
    a.send('SIMULTANEOUS_A original user');b.send('SIMULTANEOUS_B original user')
    wait_for(lambda:len(provider.bindings)==2 and final(a_file) and final(b_file))
    assert provider.bindings[0]['caller']['session']!=provider.bindings[1]['caller']['session']
    a.close();b.close();receipt['checks'].append('two overlapping controlling-PTY sessions bind distinct native identities and quit with zero exit')
    resumed=Native('resumed',a_file);resumed.send('RESUME_PUBLIC call consult once')
    wait_for(lambda:len(provider.bindings)==3 and a_file.read_text().count('AFTER_TRANSFORM')==2)
    assert provider.bindings[2]['caller']['session']==entries(a_file)[0]['id']
    resumed.send('/compact');wait_for(lambda:any(r.get('type')=='compaction' for r in entries(a_file)))
    resumed.send('AFTER_COMPACT call consult once')
    wait_for(lambda:len(provider.bindings)==4 and a_file.read_text().count('AFTER_TRANSFORM')==3)
    resumed.close();receipt['checks'].extend(['same native session resumed in a fresh controlling terminal','native interactive compact then another bound consult'])
    wait_for(lambda:bool(cli('job','list')['items']) and all(j['state']=='succeeded' for j in cli('job','list')['items']))
    receipt['capture_jobs']=[{'id':j['id'],'state':j['state']} for j in cli('job','list')['items']]
    receipt['bindings']=[{k:v for k,v in b.items() if k in ['invocation_id','caller','tip','requested_cursor','available_cursor']} for b in provider.bindings]
    assert all(b['requested_cursor']==b['available_cursor'] for b in receipt['bindings'])
    receipt['files']=[]
    for i,file in enumerate([a_file,b_file]):
        raw=file.read_bytes();assert len(raw)<262144 and b'BEFORE_TRANSFORM' not in raw and b'AFTER_TRANSFORM' in raw and b'original user' in raw
        (OUTPUT/f'session-{i}.jsonl').write_bytes(raw);receipt['files'].append(dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),session=entries(file)[0]['id']))
    receipt['status']='PASS_NATIVE_CONTROLLED_TUI'
finally:
    provider.interrupt_release.set()
    for native in children:
        if native.process.poll() is None:
            os.killpg(native.process.pid, signal.SIGKILL)
            native.process.wait(timeout=10)
        native.thread.join(timeout=5)
        native.stderr.close()
    if daemon is not None and daemon.poll() is None:
        try:
            cli('daemon', 'stop')
            daemon.wait(timeout=10)
        finally:
            if daemon.poll() is None:
                os.killpg(daemon.pid, signal.SIGKILL)
                daemon.wait(timeout=10)
    if installed:
        cli('adapter', 'uninstall', 'pi', '--agent-dir', str(profile))
    provider.close()
    receipt['provider_requests'] = provider.requests
    receipt['provider_errors'] = provider.errors
    receipt['processes_ended'] = all(n.process.poll() is not None for n in children) and (daemon is None or daemon.poll() is not None)
    assert receipt['processes_ended'] and root.parent == TEMP_ROOT and not root.is_symlink()
    shutil.rmtree(root)
    receipt['scratch_removed'] = not root.exists()
    receipt['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUTPUT / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt), flush=True)
