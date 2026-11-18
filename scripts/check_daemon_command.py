#!/usr/bin/env python3
"""Real installed CLI, alternate kernel UID, and owned-socket acceptance."""
import hashlib
import json
import os
import pathlib
import selectors
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time


def kill_group(child):
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    child.stdout.close()
    if child.stderr is not None:
        child.stderr.close()
    child.wait()


def command(argv, **kwargs):
    timeout = kwargs.pop('timeout', 20)
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, start_new_session=True, **kwargs)
    try:
        stdout, stderr = child.communicate(timeout=timeout)
        return subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
    except BaseException:
        kill_group(child)
        raise


def run_suite(argv, env, output, idle_timeout=300, timeout=1800):
    child = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, start_new_session=True)
    deadline = time.monotonic() + timeout
    idle_deadline = time.monotonic() + idle_timeout
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while True:
                remaining = min(deadline, idle_deadline) - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise subprocess.TimeoutExpired(argv, timeout if deadline <= idle_deadline else idle_timeout)
                chunk = os.read(child.stdout.fileno(), 8192)
                if not chunk:
                    break
                output.write(chunk)
                output.flush()
                idle_deadline = time.monotonic() + idle_timeout
        # A child can close its output before it exits.
        child.wait(timeout=max(0, min(deadline, idle_deadline) - time.monotonic()))
        if child.returncode:
            raise subprocess.CalledProcessError(child.returncode, argv)
    except BaseException:
        kill_group(child)
        raise
    finally:
        child.stdout.close()


def owned_daemons(base):
    expected = [os.fsencode(arg) for arg in [*base, 'daemon', 'run']]
    expected_exe = pathlib.Path(base[0]).resolve(strict=True)
    found = []
    for proc in pathlib.Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if (proc / 'cmdline').read_bytes().rstrip(b'\0').split(b'\0') == expected:
                pid = int(proc.name)
                before = identity(pid)
                if not before or before[1] == 'Z':
                    continue
                # Recheck ownership between two reads of the candidate's start time.
                if (proc / 'cmdline').read_bytes().rstrip(b'\0').split(b'\0') != expected:
                    continue
                if (proc / 'exe').resolve(strict=True) != expected_exe:
                    continue
                after = identity(pid)
                if after and after[0] == before[0] and after[1] != 'Z':
                    found.append((pid, before[0]))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    return found


def identity(pid):
    try:
        fields = pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return int(fields[19]), fields[0]
    except (FileNotFoundError, ProcessLookupError):
        return None


def kill_owned(pid, started_ticks, timeout):
    try:
        fd = os.pidfd_open(pid)
    except ProcessLookupError:
        return
    try:
        current = identity(pid)
        if current is None or current[0] != started_ticks or current[1] == 'Z':
            return
        try:
            signal.pidfd_send_signal(fd, signal.SIGKILL)
        except ProcessLookupError:
            return
        with selectors.DefaultSelector() as watcher:
            watcher.register(fd, selectors.EVENT_READ)
            assert watcher.select(timeout), 'owned process did not exit after cleanup'
    finally:
        os.close(fd)


def another_uid():
    choices = []
    if shutil.which('unshare') and shutil.which('setpriv'):
        choices.append(['unshare', '--map-auto', '--map-root-user', 'setpriv', '--reuid=1',
                        '--regid=1', '--clear-groups', '--bounding-set=-all'])
    if os.geteuid() == 0:
        choices.append(['setpriv', '--reuid=65534', '--regid=65534', '--clear-groups', '--bounding-set=-all'])
    elif shutil.which('sudo'):
        choices.append(['sudo', '-n', '-u', 'nobody', '--'])
    for prefix in choices:
        result = command([*prefix, sys.executable, '-c', 'import os; print(os.geteuid())'])
        if result.returncode == 0 and int(result.stdout) != os.geteuid():
            return prefix
    raise AssertionError('alternate UID fixture unavailable; no permission acceptance claimed')


def main():
    candidate = pathlib.Path(sys.argv[1]).resolve()
    install = '--install' in sys.argv[2:]
    with tempfile.TemporaryDirectory(prefix='akasha-installed-') as temporary:
        root = pathlib.Path(temporary)
        root.chmod(0o755)
        binary = candidate
        if install:
            result = command(['cargo', 'install', '--path', '.', '--locked', '--offline',
                              '--root', str(root / 'install')], timeout=600)
            assert result.returncode == 0, result.stderr
            binary = root / 'install/bin/akasha-otsuge'
            assert hashlib.sha256(binary.read_bytes()).digest() == hashlib.sha256(candidate.read_bytes()).digest(), 'installed binary differs from release artifact'
            environment = dict(os.environ, AKASHA_ACCEPTANCE_BINARY=str(binary))
            run_suite(['cargo', 'test', '--locked', '--test', 'daemon', '--', '--test-threads=1'],
                      environment, sys.stdout.buffer)
        config = root / 'config.toml'
        endpoint = root / 'run/service.sock'
        data = root / 'data'
        config.write_text(f'[paths]\ndata_dir={json.dumps(str(data))}\nsocket_path={json.dumps(str(endpoint))}\n')
        base = [str(binary), '--config', str(config), '--socket', str(endpoint)]
        pid = None
        original = None
        try:
            started = command([*base, 'daemon', 'start'], env=dict(
                os.environ, MALLOC_ARENA_MAX='8', LANCE_FTS_NUM_SHARDS='7',
                LANCE_FTS_PARTITION_SIZE='64'))
            assert started.returncode == 0, started.stderr
            status = json.loads(started.stdout)
            pid = status['data']['pid']
            original = identity(pid)
            assert original and original[1] != 'Z'
            owned = owned_daemons(base)
            assert owned == [(pid, original[0])], {'expected': [(pid, original[0])], 'observed': owned}
            assert pathlib.Path(f'/proc/{pid}/exe').resolve() == binary.resolve()
            native_environment = pathlib.Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
            for setting in (b'MALLOC_ARENA_MAX=2', b'LANCE_FTS_NUM_SHARDS=1',
                            b'LANCE_FTS_PARTITION_SIZE=32'):
                assert setting in native_environment, setting
            assert endpoint.stat().st_mode & 0o777 == 0o600
            assert endpoint.parent.stat().st_mode & 0o777 == 0o700
            assert data.stat().st_mode & 0o777 == 0o700
            inodes = set()
            for fd in pathlib.Path(f'/proc/{pid}/fd').iterdir():
                try:
                    target = os.readlink(fd)
                except FileNotFoundError:
                    continue
                if target.startswith('socket:['):
                    inodes.add(target[8:-1])
            for table in ('tcp', 'tcp6'):
                for line in pathlib.Path(f'/proc/{pid}/net/{table}').read_text().splitlines()[1:]:
                    assert line.split()[9] not in inodes, 'daemon owns an unexpected TCP socket'
            prefix = another_uid()
            control_path = root / 'control.sock'
            with socket.socket(socket.AF_UNIX) as control:
                control.bind(str(control_path))
                control_path.chmod(0o666)
                control.listen(2)
                control.settimeout(10)
                script = r'''
import errno,json,os,pathlib,socket,sys
control,endpoint,data=sys.argv[1:]
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(3);s.connect(control);s.sendall(b'control')
assert pathlib.Path(control).exists()
for path,kind in [(endpoint,'socket'),(data,'data')]:
 try:
  if kind=='socket':
   with socket.socket(socket.AF_UNIX) as s: s.settimeout(3);s.connect(path)
  else: pathlib.Path(path).read_bytes()
 except PermissionError as e: assert e.errno in (errno.EACCES,errno.EPERM)
 else: raise AssertionError(kind+' was accessible')
print(json.dumps({'private_paths_denied':True,'uid':os.geteuid()}))
'''
                # Inherit this accessible cwd; TMPDIR's ancestors may be private.
                denied = command([*prefix, sys.executable, '-c', script, 'control.sock',
                                  str(endpoint.relative_to(root)),
                                  str((data / 'authority/format.json').relative_to(root))],
                                 cwd=root)
                assert denied.returncode == 0, denied.stderr
                with control.accept()[0] as connection:
                    _, peer_uid, _ = struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                    assert peer_uid != os.geteuid(), 'control did not reach a different kernel UID'
                    assert connection.recv(7) == b'control'
                # The same accessible control proves this is the peer-credential check,
                # after deliberately relaxing only the disposable fixture's socket modes.
                endpoint.parent.chmod(0o755)
                endpoint.chmod(0o666)
                peer_script = r'''
import socket,sys
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(3);s.connect(sys.argv[1]);assert s.recv(1)==b''
print('peer_denied')
'''
                try:
                    peer = command([*prefix, sys.executable, '-c', peer_script,
                                    str(endpoint.relative_to(root))], cwd=root)
                    assert peer.returncode == 0 and peer.stdout.strip() == 'peer_denied', peer.stderr
                finally:
                    endpoint.chmod(0o600)
                    endpoint.parent.chmod(0o700)
            stopped = command([*base, 'daemon', 'stop'])
            assert stopped.returncode == 0, stopped.stderr
            end = time.monotonic() + 15
            while (current := identity(pid)) and current[0] == original[0] and current[1] != 'Z':
                assert time.monotonic() < end, 'detached daemon failed to stop'
                time.sleep(0.02)
            assert not endpoint.exists()
            unavailable = command([*base, 'daemon', 'status'])
            assert unavailable.returncode == 3
            assert json.loads(unavailable.stderr)['error']['code'] == 'DAEMON_UNAVAILABLE'
            print(json.dumps({'installed': install, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                              'lifecycle': 'passed', 'private_uid_denial': 'passed', 'peer_uid_denial': 'passed',
                              'tcp_sockets': 0, 'kernel_control_uid': peer_uid}))
        finally:
            # A timed out start can detach successfully before returning its PID.
            for owned_pid, started_ticks in owned_daemons(base):
                kill_owned(owned_pid, started_ticks, timeout=10)


if __name__ == '__main__':
    main()
