import hashlib, json, pathlib, subprocess, sys, time

E = pathlib.Path(__file__).parent
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
assert head == '77a78c620a8a303b8d98d591f2f9ea57232673bb'
assert not subprocess.check_output(['git', 'status', '--porcelain'], text=True).strip()
assert pathlib.Path('target').is_symlink()
assert pathlib.Path('target').readlink() == pathlib.Path('/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target')
subprocess.run(['cargo', 'clean', '--package', 'akasha-otsuge'], check=True)
listing = subprocess.run(['cargo', 'test', '--locked', '--test', 'daemon', '--', '--list'], capture_output=True, text=True)
(E / 'mvp020-candidate-four-r2-list.log').write_text(listing.stdout + listing.stderr)
assert listing.returncode == 0
editor = [line for line in listing.stdout.splitlines() if line.startswith('editor_tests::')]
assert len(editor) == 9 and '112 tests, 0 benchmarks' in listing.stdout, editor
log = E / 'mvp020-candidate-four-r2.log'
start = time.monotonic()
command = ['cargo', 'test', '--locked', '--test', 'daemon', '--', '--test-threads=4']
with log.open('xb') as out:
    rc = subprocess.run(command, stdout=out, stderr=subprocess.STDOUT).returncode
row = {'head': head, 'command': command, 'exit_code': rc, 'seconds': time.monotonic() - start,
       'editor_tests_listed': editor, 'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(),
       'clean_after': not subprocess.check_output(['git', 'status', '--porcelain'], text=True).strip()}
(E / 'mvp020-candidate-four-r2.json').write_text(json.dumps(row, indent=2) + '\n')
print(json.dumps(row), flush=True)
sys.exit(rc)
