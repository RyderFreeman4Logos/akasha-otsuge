import collections
import datetime
import hashlib
import json
import os
import pathlib
import subprocess
import time

E = pathlib.Path(__file__).parent
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == '3b862820b62f189c34f51f45d67b90714aed4ee9'
assert not subprocess.check_output(['git', 'status', '--porcelain'])
log = E / 'mvp020-workspace-observe-r1.tests.log'
samples = E / 'mvp020-workspace-observe-r1.samples.jsonl'
started = time.monotonic()
with log.open('xb') as output, samples.open('x') as sink:
    child = subprocess.Popen(['cargo', 'test', '--workspace'], stdout=output, stderr=subprocess.STDOUT)
    previous_log_size = 0
    while child.poll() is None:
        began = time.monotonic()
        pending, seen, processes = [child.pid], set(), []
        while pending:
            pid = pending.pop()
            if pid in seen:
                continue
            seen.add(pid)
            root = pathlib.Path('/proc') / str(pid)
            try:
                name = (root / 'comm').read_text().strip()
                state = (root / 'stat').read_text().rsplit(') ', 1)[1].split()
                counts, waits = collections.Counter(), []
                for task in (root / 'task').iterdir():
                    try:
                        pending.extend(map(int, (task / 'children').read_text().split()))
                        fields = (task / 'stat').read_text().rsplit(') ', 1)[1].split()
                        wchan = (task / 'wchan').read_text().strip()
                        counts[(fields[0], wchan)] += 1
                        syscall = (task / 'syscall').read_text().split()
                        number = int(syscall[0]) if syscall and syscall[0].lstrip('-').isdigit() else None
                        if fields[0] == 'D' or number in (74, 75):
                            fd = int(syscall[1], 0) if number in (74, 75) else None
                            basename = pathlib.Path(os.readlink(root / 'fd' / str(fd))).name if fd is not None else None
                            waits.append({'tid': int(task.name), 'start_ticks': fields[19], 'state': fields[0],
                                          'wchan': wchan, 'syscall': number, 'fd': fd, 'fd_basename': basename,
                                          'schedstat': (task / 'schedstat').read_text().strip(),
                                          'io': (task / 'io').read_text().splitlines()})
                    except (OSError, ValueError):
                        continue
                processes.append({'pid': pid, 'name': name, 'start_ticks': state[19],
                                  'thread_states': [[*key, count] for key, count in counts.items()], 'waits': waits})
            except OSError:
                continue
        size = log.stat().st_size
        progress = []
        if size != previous_log_size:
            with log.open('rb') as read:
                read.seek(previous_log_size)
                progress = [line[:220] for line in read.read().decode(errors='replace').splitlines()
                            if line.startswith(('test ', 'running ', '   Compiling', '     Running'))]
            previous_log_size = size
        row = {'elapsed_s': began - started, 'sample_cost_s': time.monotonic() - began,
               'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
               'test_log_bytes': size, 'progress': progress, 'processes': processes,
               'io_pressure': pathlib.Path('/proc/pressure/io').read_text().strip(),
               'cpu_pressure': pathlib.Path('/proc/pressure/cpu').read_text().strip(),
               'device_sdb': pathlib.Path('/sys/block/sdb/stat').read_text().strip()}
        sink.write(json.dumps(row, separators=(',', ':')) + '\n')
        sink.flush()
        time.sleep(max(0, 0.5 - (time.monotonic() - began)))
result = {'exit_code': child.returncode, 'elapsed_s': time.monotonic() - started,
          'command': ['cargo', 'test', '--workspace'], 'source_unchanged': not subprocess.check_output(['git', 'status', '--porcelain']),
          'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(),
          'samples_sha256': hashlib.sha256(samples.read_bytes()).hexdigest(),
          'limits': 'Sampled syscall residency is not proof of uninterrupted calls. No source probes, ptrace, concurrency changes, deadline changes or durability changes. Diagnostic only; canonical R4 remains failed.'}
(E / 'mvp020-workspace-observe-r1.result.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result), flush=True)
raise SystemExit(child.returncode)
