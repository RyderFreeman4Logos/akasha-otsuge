#!/usr/bin/env python3
"""Real installed CLI, alternate kernel UID, and owned-socket acceptance."""
import errno
import hashlib
import json
import os
import pathlib
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time


def command(argv, **kwargs):
    return subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, text=True,
                          timeout=kwargs.pop('timeout', 20), **kwargs)


def identity(pid):
    try:
        fields = pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        return int(fields[19]), fields[0]
    except FileNotFoundError:
        return None


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
            tests = command(['cargo', 'test', '--locked', '--test', 'daemon', '--', '--test-threads=1'],
                            env=environment, timeout=300)
            assert tests.returncode == 0, tests.stdout + tests.stderr
            print(tests.stdout, end='')
        config = root / 'config.toml'
        endpoint = root / 'run/service.sock'
        data = root / 'data'
        config.write_text(f'[paths]\ndata_dir={json.dumps(str(data))}\nsocket_path={json.dumps(str(endpoint))}\n')
        base = [str(binary), '--config', str(config), '--socket', str(endpoint)]
        pid = None
        original = None
        try:
            started = command([*base, 'daemon', 'start'])
            assert started.returncode == 0, started.stderr
            status = json.loads(started.stdout)
            pid = status['data']['pid']
            original = identity(pid)
            assert original and original[1] != 'Z'
            assert pathlib.Path(f'/proc/{pid}/exe').resolve() == binary.resolve()
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
                denied = command([*prefix, sys.executable, '-c', script, str(control_path),
                                  str(endpoint), str(data / 'authority/format.json')])
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
                    peer = command([*prefix, sys.executable, '-c', peer_script, str(endpoint)])
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
            if pid and original and (current := identity(pid)) and current[0] == original[0] and current[1] != 'Z':
                os.kill(pid, signal.SIGKILL)
                end = time.monotonic() + 10
                while (current := identity(pid)) and current[0] == original[0] and current[1] != 'Z':
                    assert time.monotonic() < end, 'owned daemon did not exit after cleanup'
                    time.sleep(0.02)


if __name__ == '__main__':
    main()
