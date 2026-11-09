import datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, tempfile, time

E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp020')
TMP = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp')
PI = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/npm-earendil-works-pi-coding-agent/1.4.0/node_modules/.bin/pi')
N = pathlib.Path('/home/obj/.pi/agent')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
tag = os.environ['AKASHA_NATIVE_TAG']
assert tag.isalnum()
output = E / f'mvp020-native-{tag}.json'
result = E / f'mvp020-native-{tag}-result.json'
assert not output.exists() and not result.exists() and PI.is_file()
assert shutil.disk_usage(TMP).free > 1024**3
before = {n: sha(N / n) for n in ['models.json', 'settings.json']}
provider = json.loads((N / 'models.json').read_text())['providers']['mp-cpi']
assert provider['apiKey'].startswith('!')
provider['baseUrl'] = 'http://127.0.0.1:48317/v1'
provider['models'] = [m for m in provider['models'] if m['id'] == 'grok-4.7']
assert len(provider['models']) == 1
os.umask(0o077)
root = pathlib.Path(tempfile.mkdtemp(prefix='a20-native-', dir=TMP))
profile = root / 'profile'
profile.mkdir()
env = dict(os.environ, TMPDIR=str(root), TMP=str(root), TEMP=str(root),
           AKASHA_NATIVE_EDITOR_HOST='pi', AKASHA_NATIVE_EDITOR_BINARY=str(PI), AKASHA_NATIVE_EDITOR_HOME=str(profile),
           AKASHA_NATIVE_EDITOR_RECEIPT=str(result))
if os.environ.get('AKASHA_PI_OBSERVE') == '1':
    env['AKASHA_NATIVE_EDITOR_BINARY'] = str(E / 'mvp020-pi-observer.py')
    env['AKASHA_PI_DIAGNOSTIC'] = str(E / f'mvp020-native-{tag}-errors.jsonl')
r = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
     'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=W, text=True).strip(),
     'dirty_diff_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD', '--binary'], cwd=W)).hexdigest(),
     'source': 'controlled Caller and synthetic evidence; actual authenticated Pi Runner',
     'native_binary': str(PI.resolve()), 'native_binary_sha256': sha(PI),
     'provider': 'mp-cpi', 'model': 'grok-4.7', 'endpoint': 'http://127.0.0.1:48317/v1',
     'scratch': str(root), 'status': 'FAILED'}
child = None
start = time.monotonic()
try:
    (profile / 'models.json').write_text(json.dumps({'providers': {'mp-cpi': provider}}))
    (profile / 'settings.json').write_text(json.dumps({'defaultProvider': 'mp-cpi', 'defaultModel': 'grok-4.7', 'defaultThinkingLevel': 'off'}))
    r['host_version'] = subprocess.check_output([str(PI), '--version'], env=env, text=True, timeout=15).strip()
    cmd = ['cargo', 'test', '--locked', '--test', 'daemon', 'editor_tests::native_editor_actual', '--', '--exact', '--ignored', '--nocapture']
    r['command'] = cmd
    output.write_text(json.dumps(r, indent=2) + '\n')
    child = subprocess.Popen(cmd, cwd=W, env=env, stdin=subprocess.DEVNULL, start_new_session=True)
    r['exit_code'] = child.wait(timeout=7200)
    assert r['exit_code'] == 0 and result.exists()
    receipt = json.loads(result.read_text())
    assert receipt['status'] == 'PASS_REAL_NATIVE_EDITOR'
    r['status'] = receipt['status']
    r['patch_commit_seq'] = receipt['job']['result']['patch_receipt']['commit_seq']
finally:
    if child and child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=30)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
    if result.exists():
        r['result_sha256'] = sha(result)
    binary = pathlib.Path(env['CARGO_TARGET_DIR']) / 'debug/akasha-otsuge'
    if binary.exists():
        r['binary_sha256'] = sha(binary)
    alive = []
    for proc in pathlib.Path('/proc').iterdir():
        if not proc.name.isdecimal():
            continue
        try:
            cwd = (proc / 'cwd').resolve()
            state = (proc / 'stat').read_text().rsplit(') ', 1)[1].split()[0]
            if state != 'Z' and (cwd == root or root in cwd.parents):
                alive.append(int(proc.name))
        except OSError:
            pass
    r['owned_live_processes'] = alive
    r['native_settings_unchanged'] = before == {n: sha(N / n) for n in before}
    r['scratch_allocated_bytes'] = sum(p.lstat().st_blocks * 512 for p in root.rglob('*') if p.is_file() and not p.is_symlink())
    if not alive:
        assert root.parent == TMP and not root.is_symlink()
        shutil.rmtree(root)
    r['scratch_removed'] = not root.exists()
    r['ssd_free_bytes_after'] = shutil.disk_usage(TMP).free
    r['seconds'] = time.monotonic() - start
    r['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    output.write_text(json.dumps(r, indent=2) + '\n')
    print(json.dumps(r), flush=True)
    assert not alive and r['native_settings_unchanged']
