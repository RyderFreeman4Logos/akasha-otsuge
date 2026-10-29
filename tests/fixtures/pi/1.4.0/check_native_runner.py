"""Native Pi protocol qualification with a synthetic, loopback-only provider."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from native_provider import Provider

A = Path(os.environ['AKASHA_BINARY']).resolve(strict=True)
PI = Path(os.environ['AKASHA_PI_BINARY']).resolve(strict=True)
TEST = Path(os.environ['AKASHA_PI_TEST']).resolve(strict=True)
TEMP_ROOT = Path(os.environ['TMPDIR']).resolve(strict=True)
OUTPUT = Path(sys.argv[1]).resolve()
OUTPUT.mkdir(parents=True, exist_ok=False)
assert shutil.disk_usage(TEMP_ROOT).free >= 128 * 1024 * 1024
os.umask(0o077)
root = Path(tempfile.mkdtemp(prefix='akasha-native-pi-runner-', dir=TEMP_ROOT))
profile = root / 'agent'
profile.mkdir(mode=0o700)
env = dict(os.environ, PATH=os.environ['AKASHA_NATIVE_PATH'], TMPDIR=str(root), TMP=str(root), TEMP=str(root),
           AKASHA_PI_BINARY=str(PI), AKASHA_PI_HOME=str(profile), AKASHA_PI_ACCEPTANCE_SECONDS='180')
provider = Provider()
installed = False
child = None
receipt = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'scratch': str(root),
           'provider_api': provider.api,
           'kind': 'actual Pi CLI RPC with controlled local SSE; no authenticated model', 'status': 'failed',
           'binary_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [A, PI, TEST]}, 'cases': []}


def cli(*args):
    result = subprocess.run([str(A), *args], cwd=root, env=env, capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stderr[:1000]


def owned_processes():
    owned = []
    for path in Path('/proc').iterdir():
        if not path.name.isdecimal():
            continue
        try:
            cwd = Path(os.readlink(path / 'cwd').removesuffix(' (deleted)'))
            fields = (path / 'stat').read_text().rsplit(') ', 1)[1].split()
            if (cwd == root or root in cwd.parents) and fields[0] != 'Z':
                owned.append((int(path.name), fields[19]))
        except OSError:
            pass
    return owned


try:
    receipt['other_cli_absent'] = {n: shutil.which(n, path=env['PATH']) is None for n in ['claude', 'codex', 'hermes']}
    assert all(receipt['other_cli_absent'].values())
    provider.configure(profile)
    cli('--socket', str(root / 'unused.sock'), 'adapter', 'install', 'pi', '--binary', str(PI), '--agent-dir', str(profile))
    installed = True
    for case, test in [('roundtrip', 'installed_native_runner_roundtrip'), ('retry', 'installed_native_runner_roundtrip'),
                       ('denied', 'installed_native_rejects_unissued_context'),
                       ('cancel', 'installed_native_cancellation_ends_executor_after_context_tool'),
                       ('forbidden', 'installed_native_rejects_unadvertised_tool')]:
        provider.case, provider.requests, provider.errors = case, [], []
        with (OUTPUT / (case + '.log')).open('xb') as log:
            child = subprocess.Popen([str(TEST), 'runner::' + test, '--ignored', '--exact', '--nocapture'],
                                     cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
            status = child.wait(timeout=220)
        row = {'case': case, 'exit_code': status, 'requests': provider.requests[:], 'errors': provider.errors[:],
               'residual_before_cleanup': owned_processes()}
        receipt['cases'].append(row)
        assert status == 0 and not row['errors'] and not row['residual_before_cleanup'], row
        if case == 'retry':
            assert len(row['requests']) == 6, 'one transient failure plus five successful provider requests'
    receipt['status'] = 'PASS_NATIVE_CONTROLLED_RUNNER'
finally:
    if child is not None and child.poll() is None:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=10)
    for pid, start in owned_processes():
        try:
            pidfd = os.pidfd_open(pid)
            try:
                fields = Path(f'/proc/{pid}/stat').read_text().rsplit(') ', 1)[1].split()
                if fields[19] == start:
                    signal.pidfd_send_signal(pidfd, signal.SIGKILL)
            finally:
                os.close(pidfd)
        except (FileNotFoundError, ProcessLookupError):
            pass
    deadline = time.monotonic() + 5
    while owned_processes() and time.monotonic() < deadline:
        time.sleep(.05)
    receipt['processes_ended'] = not owned_processes()
    assert receipt['processes_ended']
    if installed:
        cli('adapter', 'uninstall', 'pi', '--agent-dir', str(profile))
    provider.close()
    assert root.parent == TEMP_ROOT and not root.is_symlink()
    shutil.rmtree(root)
    receipt['scratch_removed'] = not root.exists()
    receipt['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUTPUT / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt), flush=True)
