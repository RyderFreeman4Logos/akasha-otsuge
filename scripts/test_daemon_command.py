import io
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from check_daemon_command import command, identity, kill_owned, owned_daemons, run_suite


class CommandOwnership(unittest.TestCase):
    def test_suite_streams_both_outputs_and_keeps_running_with_progress(self):
        log = io.BytesIO()
        script = ('import sys,time\n'
                  'for i in range(6):\n'
                  ' print(i, file=sys.stderr if i % 2 else sys.stdout, flush=True)\n'
                  ' time.sleep(0.25)')
        run_suite([sys.executable, '-c', script], os.environ, log, idle_timeout=1, timeout=8)
        self.assertEqual(log.getvalue().splitlines(), [str(i).encode() for i in range(6)])

    def test_suite_checks_exit_status_and_absolute_limit_despite_output(self):
        log = io.BytesIO()
        with self.assertRaises(subprocess.CalledProcessError) as failed:
            run_suite([sys.executable, '-c', 'raise SystemExit(7)'], os.environ, log)
        self.assertEqual(failed.exception.returncode, 7)
        script = 'import time\nwhile True: print("running", flush=True); time.sleep(0.1)'
        with self.assertRaises(subprocess.TimeoutExpired) as expired:
            run_suite([sys.executable, '-c', script], os.environ, log, idle_timeout=5, timeout=1)
        self.assertEqual(expired.exception.timeout, 1)
        self.assertIn(b'running', log.getvalue())

    def test_owned_census_identity_survives_normal_scheduling_changes(self):
        with tempfile.TemporaryDirectory(prefix='akasha-command-census-') as root:
            proc = pathlib.Path(root) / '123'
            proc.mkdir()
            base = [sys.executable, '--config', str(pathlib.Path(root) / 'config')]
            (proc / 'cmdline').write_bytes(
                b'\0'.join(os.fsencode(arg) for arg in [*base, 'daemon', 'run']) + b'\0')
            (proc / 'exe').symlink_to(sys.executable)
            with patch('check_daemon_command.pathlib.Path.iterdir', return_value=[proc]), \
                    patch('check_daemon_command.identity', side_effect=[
                        (111, 'S'), (111, 'R'), (111, 'R'), (111, 'S'),
                        (111, 'S'), (112, 'S'), (111, 'Z'), None]):
                original = owned_daemons(base)
                self.assertEqual(len(original), 1)
                self.assertEqual(owned_daemons(base), original)
                self.assertEqual(owned_daemons(base), [])
                self.assertEqual(owned_daemons(base), [])
                self.assertEqual(owned_daemons(base), [])

            expected = (proc / 'cmdline').read_bytes()
            with patch('check_daemon_command.pathlib.Path.iterdir', return_value=[proc]), \
                    patch('check_daemon_command.identity', return_value=(112, 'S')):
                with patch('pathlib.Path.read_bytes', side_effect=[expected, b'unrelated\0']):
                    self.assertEqual(owned_daemons(base), [])
                (proc / 'exe').unlink()
                self.assertEqual(owned_daemons(base), [])

    def test_proc_identity_disappearance_is_terminal_but_access_failure_is_not(self):
        for error in (FileNotFoundError(), ProcessLookupError()):
            with patch('check_daemon_command.pathlib.Path.read_text', side_effect=error):
                self.assertIsNone(identity(123))
        with patch('check_daemon_command.pathlib.Path.read_text', side_effect=PermissionError()):
            with self.assertRaises(PermissionError):
                identity(123)

    def test_timeout_collects_the_owned_subprocess_group(self):
        self.check_group_cleanup(lambda argv: command(argv, timeout=2))

    def test_cleanup_uses_the_witnessed_process_and_waits_for_exit(self):
        with subprocess.Popen([sys.executable, '-c', 'input()'], stdin=subprocess.PIPE) as child:
            started, _ = identity(child.pid)
            kill_owned(child.pid, started + 1, timeout=1)
            self.assertIsNone(child.poll())
            with patch('signal.pidfd_send_signal', side_effect=PermissionError()):
                with self.assertRaises(PermissionError):
                    kill_owned(child.pid, started, timeout=1)
            kill_owned(child.pid, started, timeout=2)
            self.assertEqual(child.poll(), -signal.SIGKILL)

    def test_cleanup_accepts_exit_between_identity_check_and_signal(self):
        with subprocess.Popen([sys.executable, '-c', 'input()'], stdin=subprocess.PIPE) as child:
            start, state = identity(child.pid)

            def exit_after_read(pid):
                self.assertEqual(pid, child.pid)
                child.stdin.write(b'\n')
                child.stdin.flush()
                self.assertEqual(child.wait(timeout=2), 0)
                return start, state

            with patch('check_daemon_command.identity', side_effect=exit_after_read):
                kill_owned(child.pid, start, timeout=1)
            kill_owned(child.pid, start, timeout=1)

    def test_suite_idle_timeout_collects_the_owned_subprocess_group(self):
        self.check_group_cleanup(lambda argv: run_suite(argv, os.environ, io.BytesIO(), idle_timeout=2))

    def check_group_cleanup(self, run):
        with tempfile.TemporaryDirectory(prefix='akasha-command-timeout-') as root:
            witness = pathlib.Path(root) / 'child.json'
            script = '''
import json,pathlib,subprocess,sys,time
p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])
fields=pathlib.Path(f'/proc/{p.pid}/stat').read_text().rsplit(')',1)[1].split()
pathlib.Path(sys.argv[1]).write_text(json.dumps([p.pid,int(fields[19])]))
time.sleep(60)
'''
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    run([sys.executable, '-c', script, str(witness)])
                pid, start = json.loads(witness.read_text())
                end = time.monotonic() + 2
                while (current := identity(pid)) and current[0] == start and current[1] != 'Z':
                    if time.monotonic() >= end:
                        self.fail('timed command left its child running')
                    time.sleep(0.01)
            finally:
                if witness.exists():
                    pid, start = json.loads(witness.read_text())
                    kill_owned(pid, start, timeout=2)


if __name__ == '__main__':
    unittest.main()
