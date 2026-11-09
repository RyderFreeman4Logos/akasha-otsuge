import datetime, hashlib, json, pathlib, subprocess, time

E = pathlib.Path(__file__).parent
cases = [
    ('base-default', pathlib.Path('/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge'), '460934961009ff41ae1b3def573e81dda4fa7d0e', []),
    ('candidate-four', pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp020'), '77a78c620a8a303b8d98d591f2f9ea57232673bb', ['--', '--test-threads=4']),
]
for name, cwd, head, args in cases:
    git = lambda *a: subprocess.check_output(['git', *a], cwd=cwd, text=True).strip()
    assert git('rev-parse', 'HEAD') == head and not git('status', '--porcelain')
    log = E / f'mvp020-topology-{name}.log'
    command = ['cargo', 'test', '--locked', '--test', 'daemon', *args]
    row = {'head': head, 'tree': git('rev-parse', 'HEAD^{tree}'), 'cwd': str(cwd),
           'command': command, 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    start = time.monotonic()
    with log.open('xb') as output:
        rc = subprocess.run(command, cwd=cwd, stdout=output, stderr=subprocess.STDOUT).returncode
    assert git('rev-parse', 'HEAD') == head and not git('status', '--porcelain')
    row.update(exit_code=rc, seconds=time.monotonic() - start,
               log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(), clean_before_after=True)
    (E / f'mvp020-topology-{name}.json').write_text(json.dumps(row, indent=2) + '\n')
    print(json.dumps(row), flush=True)
