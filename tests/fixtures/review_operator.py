"""Drive the real review CLI through a kernel PTY with synthetic operator input."""
import errno
import json
import os
import pty
import re
import select
import signal
import subprocess
import sys
import time

argv = json.loads(sys.argv[1]) + ["review", "apply", sys.argv[2], "--idempotency-key", sys.argv[3]]
pid, fd = pty.fork()
if pid == 0:
    os.execv(argv[0], argv)
data = b""
sent = False
status = None
ended = 0
deadline = time.monotonic() + 30
try:
    while time.monotonic() < deadline:
        if select.select([fd], [], [], 0.1)[0]:
            try:
                chunk = os.read(fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                break
            if not chunk:
                break
            data += chunk
            if len(data) > 2 * 1024 * 1024:
                raise RuntimeError("operator output exceeded bound")
            match = re.search(rb"Type (confirm [0-9a-f]{64}) to apply:", data)
            if match and not sent:
                base = json.loads(sys.argv[1])
                if sys.argv[4] == "confirm-revoke":
                    subprocess.run(base + ["access", "revoke"], check=True, capture_output=True)
                if sys.argv[4] == "confirm-reload":
                    config = base[base.index("--config") + 1]
                    with open(config, "a") as target:
                        target.write("\n[review]\nmode='automatic'\n")
                    subprocess.run(base + ["config", "reload"], check=True, capture_output=True)
                os.write(fd, (match[1] if sys.argv[4].startswith("confirm") else b"decline") + b"\n")
                sent = True
        ended, observed_status = os.waitpid(pid, os.WNOHANG)
        if ended:
            status = observed_status
            break
    else:
        raise TimeoutError("operator CLI did not terminate")
    if not ended:
        _, status = os.waitpid(pid, 0)
    response = None
    for line in data.decode().splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and "schema_version" in value:
            response = value
    print(json.dumps({"exit_code": os.waitstatus_to_exitcode(status), "response": response, "transcript": data.decode()}))
finally:
    os.close(fd)
    if status is None:
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
