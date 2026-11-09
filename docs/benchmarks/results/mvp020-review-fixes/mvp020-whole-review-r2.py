import datetime, hashlib, json, os, pathlib, signal, subprocess, time

E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp020')
gate = json.loads((E / 'mvp020-canonical-r2-verified.json').read_text())
git = lambda *a: subprocess.check_output(['git', *a], cwd=W, text=True).strip()
assert gate['head'] == git('rev-parse', 'HEAD') and gate['tree'] == git('rev-parse', 'HEAD^{tree}')
assert gate['clean_before_after'] and not git('status', '--porcelain')
pin = json.loads((E / 'mvp020-review-runtime-r2.json').read_text())
assert hashlib.sha256(pathlib.Path(pin['binary']).read_bytes()).hexdigest() == pin['binary_sha256']
assert hashlib.sha256(pathlib.Path(pin['companion']).read_bytes()).hexdigest() == pin['companion_sha256']
report = E / 'mvp020-whole-review-r2.md'
assert not report.exists()
cmd = [pin['binary'], 'exec', '--sandbox', 'read-only', '--ephemeral', '--json', '--color', 'never',
       '--cd', str(W), '--output-last-message', str(report), '-']
start = time.monotonic()
with (E / 'mvp020-whole-review-r2-brief.md').open('rb') as brief, (E / 'mvp020-whole-review-r2.stdout').open('xb') as out, (E / 'mvp020-whole-review-r2.stderr').open('xb') as err:
    child = subprocess.Popen(cmd, stdin=brief, stdout=out, stderr=err, start_new_session=True)
    try:
        rc = child.wait(timeout=3600)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=30)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        rc = 124
events = []
for line in (E / 'mvp020-whole-review-r2.stdout').open():
    try:
        value = json.loads(line)
    except ValueError:
        continue
    if value.get('type') in ['thread.started', 'turn.completed', 'turn.failed', 'error']:
        events.append(value)
receipt = {'exit_code': rc, 'seconds': time.monotonic() - start, 'events': events,
           'head': git('rev-parse', 'HEAD'), 'tree': git('rev-parse', 'HEAD^{tree}'),
           'clean_after': not git('status', '--porcelain'), 'report_exists': report.exists(),
           'report_sha256': hashlib.sha256(report.read_bytes()).hexdigest() if report.exists() else None,
           'transport': 'independent Codex exec; Hermes configured endpoint unavailable',
           'utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
assert hashlib.sha256(pathlib.Path(pin['binary']).read_bytes()).hexdigest() == pin['binary_sha256']
assert hashlib.sha256(pathlib.Path(pin['companion']).read_bytes()).hexdigest() == pin['companion_sha256']
receipt['runtime'] = pin
(E / 'mvp020-whole-review-r2.exit.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt), flush=True)
raise SystemExit(rc)
