"""Actual Pi CLI and daemon against an explicitly synthetic local model fixture."""
import datetime
import sys
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import uuid

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
           'kind': 'actual Pi CLI lifecycle with controlled local SSE; no authenticated model',
           'akasha_sha256': hashlib.sha256(A.read_bytes()).hexdigest(), 'status': 'failed', 'checks': []}
children = []
daemon = None
installed = False


def cli(*args):
    result = subprocess.run([*base, *args], cwd=root, env=env, capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr[:1000]
    return json.loads(result.stdout)['data']


class Native:
    def __init__(self, name, session):
        self.events = queue.Queue()
        self.pending = []
        self.trace = []
        self.stderr = (root / (name + '.stderr')).open('wb')
        args = [str(PI), '--mode', 'rpc', '--session', str(session), '--no-builtin-tools', '--no-extensions',
                '--no-mcp', '--no-skills', '--no-prompt-templates', '--no-themes', '--no-context-files',
                '--no-approve', '-e', str(profile / 'extensions/akasha-otsuge/index.ts'),
                '-e', str(E / 'lifecycle.ts'), '--tools', 'akasha_consult']
        self.process = subprocess.Popen(args, cwd=workspace, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=self.stderr, start_new_session=True)
        children.append(self)
        def output():
            try:
                for line in self.process.stdout:
                    assert len(line) <= 262144
                    self.events.put(json.loads(line))
            finally:
                self.events.put(None)
        self.thread = threading.Thread(target=output, daemon=True)
        self.thread.start()

    def send(self, kind, **fields):
        request = {'id': uuid.uuid4().hex, 'type': kind, **fields}
        self.process.stdin.write((json.dumps(request) + '\n').encode())
        self.process.stdin.flush()
        return request['id']

    def receive(self, predicate):
        for index, value in enumerate(self.pending):
            if predicate(value):
                return self.pending.pop(index)
        deadline = time.monotonic() + 40
        while True:
            value = self.events.get(timeout=max(0, deadline - time.monotonic()))
            assert value is not None, 'native EOF'
            self.trace.append(value['type'])
            assert value['type'] != 'extension_error', value
            if predicate(value):
                return value
            self.pending.append(value)

    def response(self, request):
        result = self.receive(lambda value: value.get('id') == request and value['type'] == 'response')
        assert result['success'], result
        return result.get('data')

    def request(self, kind, **fields):
        return self.response(self.send(kind, **fields))

    def settled(self, aborted=False):
        value = self.receive(lambda value: value['type'] == 'agent_settled')
        assert value['aborted'] == aborted, value

    def prompt(self, message):
        assert self.request('prompt', message=message)['disposition'] == 'started'
        self.settled()

    def close(self):
        if not self.process.stdin.closed:
            self.process.stdin.close()
        assert self.process.wait(timeout=15) == 0
        self.thread.join(timeout=5)
        assert not self.thread.is_alive()
        self.stderr.close()


try:
    receipt['other_cli_absent'] = {n: shutil.which(n, path=env['PATH']) is None for n in ['claude', 'codex', 'hermes']}
    assert all(receipt['other_cli_absent'].values())
    provider.configure(profile)
    daemon_log = (root / 'daemon.log').open('wb')
    daemon = subprocess.Popen([*base, 'daemon', 'run'], cwd=root, env=env, stdin=subprocess.DEVNULL,
                              stdout=daemon_log, stderr=daemon_log, start_new_session=True)
    deadline = time.monotonic() + 10
    while not sock.exists():
        assert daemon.poll() is None and time.monotonic() < deadline
        time.sleep(.02)
    cli('adapter', 'install', 'pi', '--binary', str(PI), '--agent-dir', str(profile))
    installed = True
    a = Native('a', root / 'a.jsonl')
    b = Native('b', root / 'b.jsonl')
    ra = a.send('prompt', message='SIMULTANEOUS_A original user')
    rb = b.send('prompt', message='SIMULTANEOUS_B original user')
    assert a.response(ra)['disposition'] == b.response(rb)['disposition'] == 'started'
    a.settled()
    b.settled()
    assert len(provider.bindings) == 2
    assert provider.bindings[0]['caller']['session'] != provider.bindings[1]['caller']['session']
    receipt['checks'].append('overlapping native sessions bound independently')
    original = a.request('get_state')
    entries = a.request('get_entries')['entries']
    first_user = next(e['id'] for e in entries if e['type'] == 'message' and e['message']['role'] == 'user')
    assert any(e['type'] == 'context_edit' for e in entries)
    assert a.request('prompt', message='/accept-reload')['disposition'] == 'handled'
    a.prompt('AFTER_RELOAD call consult once')
    assert a.request('prompt', message='/accept-tree ' + first_user)['disposition'] == 'handled'
    a.prompt('BRANCH_PUBLIC call consult once')
    a.request('compact')
    a.request('clone')
    fork = a.request('get_state')
    assert fork['sessionId'] != original['sessionId']
    a.prompt('FORK_PUBLIC call consult once')
    a.request('switch_session', sessionPath=original['sessionFile'])
    a.prompt('RESUME_PUBLIC call consult once')
    receipt['checks'].extend(['reload', 'tree branch', 'native compaction', 'clone fork', 'resume'])
    interruption = a.send('prompt', message='INTERRUPT_PENDING')
    assert a.response(interruption)['disposition'] == 'started'
    assert provider.interrupt_seen.wait(8)
    a.request('abort')
    a.settled(aborted=True)
    provider.interrupt_release.set()
    a.close()
    b.close()
    receipt['checks'].append('interrupted native run settles before shutdown')
    deadline = time.monotonic() + 20
    while True:
        jobs = cli('job', 'list')['items']
        assert jobs and all(j['state'] in ['queued', 'running', 'succeeded'] for j in jobs), jobs
        if all(j['state'] == 'succeeded' for j in jobs):
            break
        assert time.monotonic() < deadline
        time.sleep(.05)
    receipt['capture_jobs'] = [{'id': j['id'], 'state': j['state']} for j in jobs]
    files = sorted(set([original['sessionFile'], fork['sessionFile'], str(root / 'b.jsonl')]))
    receipt['files'] = []
    for index, file in enumerate(files):
        raw = Path(file).read_bytes()
        assert len(raw) <= 262144
        (OUTPUT / f'session-{index}.jsonl').write_bytes(raw)
        rows = [json.loads(line) for line in raw.splitlines()]
        assert b'BEFORE_TRANSFORM' not in raw and b'AFTER_TRANSFORM' in raw
        assert any(e['type'] == 'context_edit' for e in rows)
        receipt['files'].append({'path': file, 'sha256': hashlib.sha256(raw).hexdigest(),
                                 'session': rows[0]['id'], 'rows': len(rows)})
    receipt['bindings'] = [{'invocation_id': b['invocation_id'], 'caller': b['caller'], 'tip': b['tip'],
                            'requested_cursor': b['requested_cursor'], 'available_cursor': b['available_cursor']}
                           for b in provider.bindings]
    assert all(b['requested_cursor'] == b['available_cursor'] for b in receipt['bindings'])
    assert not provider.errors, provider.errors
    receipt['checks'].extend(['final transform persisted once', 'context edits retain original entries', 'durable capture jobs drained'])
    receipt['status'] = 'PASS_NATIVE_CONTROLLED_CALLER'
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
    receipt['provider_errors'] = provider.errors
    receipt['processes_ended'] = all(n.process.poll() is not None for n in children) and (daemon is None or daemon.poll() is not None)
    receipt['native_event_counts'] = [dict(Counter(n.trace)) for n in children]
    assert receipt['processes_ended'] and root.parent == TEMP_ROOT and not root.is_symlink()
    shutil.rmtree(root)
    receipt['scratch_removed'] = not root.exists()
    receipt['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUTPUT / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt), flush=True)
