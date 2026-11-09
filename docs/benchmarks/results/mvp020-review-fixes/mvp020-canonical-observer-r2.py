import datetime, hashlib, json, os, pathlib, subprocess, tempfile, time

E = pathlib.Path(__file__).parent
TMP = pathlib.Path(os.environ['TMPDIR'])
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
assert head == '77a78c620a8a303b8d98d591f2f9ea57232673bb'
selector = 'canonical just check, unchanged default topology'
path = E / 'mvp020-canonical-r2-samples.jsonl'
with path.open('x') as samples, tempfile.TemporaryDirectory(prefix='a20-io-probe-', dir=TMP) as scratch:
    root = pathlib.Path(scratch)
    def emit(value):
        samples.write(json.dumps({'monotonic': time.monotonic(), **value}) + '\n')
        samples.flush()
    emit({'phase': 'fsync_start', 'head': head, 'root': scratch})
    with (root / 'owned-probe').open('xb') as file:
        file.write(b'owned diagnostic\n')
        file.flush()
        start = time.monotonic()
        os.fsync(file.fileno())
        emit({'phase': 'file_fsync_done', 'seconds': time.monotonic() - start})
    fd = os.open(root, os.O_DIRECTORY)
    start = time.monotonic()
    os.fsync(fd)
    os.close(fd)
    emit({'phase': 'directory_fsync_done', 'seconds': time.monotonic() - start})
    command = ['just', 'check']
    log = E / 'mvp020-canonical-r2.gate.log'
    with log.open('xb') as output:
        child = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT)
        emit({'phase': 'selector_started', 'pid': child.pid, 'command': command})
        while child.poll() is None:
            pending = [child.pid]
            seen = set()
            processes = []
            while pending:
                pid = pending.pop()
                if pid in seen:
                    continue
                seen.add(pid)
                proc = pathlib.Path('/proc') / str(pid)
                try:
                    name = (proc / 'comm').read_text().strip()
                    threads = []
                    for task in (proc / 'task').iterdir():
                        pending.extend(map(int, (task / 'children').read_text().split()))
                        try:
                            state = (task / 'stat').read_text().rsplit(') ', 1)[1].split()
                            wait = (task / 'wchan').read_text().strip()
                            if state[0] == 'D' or wait not in ['futex_wait_queue', 'ep_poll', 'do_wait', '0']:
                                threads.append({'tid': task.name, 'state': state[0], 'wait': wait})
                        except OSError:
                            pass
                    processes.append({'pid': pid, 'name': name, 'threads': threads})
                except OSError:
                    pass
            emit({'phase': 'sample', 'processes': processes,
                  'io_pressure': pathlib.Path('/proc/pressure/io').read_text().strip()})
            time.sleep(1)
    emit({'phase': 'selector_done', 'exit_code': child.returncode,
          'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest()})
print(json.dumps({'selector_exit': child.returncode, 'samples': str(path), 'scratch_removed': not root.exists()}))
raise SystemExit(child.returncode)
