import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import unittest

from check_daemon_command import command, identity


class CommandOwnership(unittest.TestCase):
    def test_timeout_collects_the_owned_subprocess_group(self):
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
                    command([sys.executable, '-c', script, str(witness)], timeout=2)
                pid, start = json.loads(witness.read_text())
                end = time.monotonic() + 2
                while (current := identity(pid)) and current[0] == start and current[1] != 'Z':
                    if time.monotonic() >= end:
                        self.fail('timed command left its child running')
                    time.sleep(0.01)
            finally:
                if witness.exists():
                    pid, start = json.loads(witness.read_text())
                    if (current := identity(pid)) and current[0] == start and current[1] != 'Z':
                        os.kill(pid, signal.SIGKILL)
                        end = time.monotonic() + 2
                        while (current := identity(pid)) and current[0] == start and current[1] != 'Z':
                            assert time.monotonic() < end
                            time.sleep(0.01)


if __name__ == '__main__':
    unittest.main()
