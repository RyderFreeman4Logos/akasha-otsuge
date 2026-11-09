import datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, tempfile, time, tomllib

E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp020')
TMP = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp')
N = pathlib.Path('/home/obj/.codex')
pin = json.loads((E / 'mvp016-codex-pin.json').read_text())
binary = pathlib.Path(pin['binary'])
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(binary) == pin['binary_sha256']
output = E / 'mvp020-native-codexr1.json'
result = E / 'mvp020-native-codexr1-result.json'
assert not output.exists() and not result.exists()
assert shutil.disk_usage(TMP).free > 1024**3
before = sha(N / 'config.toml')
config = tomllib.loads((N / 'config.toml').read_text())
os.umask(0o077)
root = pathlib.Path(tempfile.mkdtemp(prefix='a20-codex-', dir=TMP))
env = dict(os.environ, TMPDIR=str(root), TMP=str(root), TEMP=str(root),
           AKASHA_NATIVE_EDITOR_HOST='codex', AKASHA_NATIVE_EDITOR_BINARY=str(binary),
           AKASHA_NATIVE_EDITOR_HOME=str(N), AKASHA_NATIVE_EDITOR_RECEIPT=str(result))
r = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
     'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=W, text=True).strip(),
     'dirty_diff_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD', '--binary'], cwd=W)).hexdigest(),
     'source': 'controlled Caller and synthetic evidence; actual authenticated Codex Runner',
     'native_binary': str(binary), 'native_binary_sha256': sha(binary), 'host_version': pin['version'],
     'configured_model': config.get('model'), 'configured_provider': config.get('model_provider', 'openai'),
     'scratch': str(root), 'status': 'FAILED'}
child = None
start = time.monotonic()
try:
    cmd = ['cargo', 'test', '--locked', '--test', 'daemon', 'editor_tests::native_editor_actual', '--', '--exact', '--ignored', '--nocapture']
    r['command'] = cmd
    output.write_text(json.dumps(r, indent=2) + '\n')
    child = subprocess.Popen(cmd, cwd=W, env=env, stdin=subprocess.DEVNULL, start_new_session=True)
    r['exit_code'] = child.wait(timeout=7200)
    assert r['exit_code'] == 0 and result.exists()
    receipt = json.loads(result.read_text())
    assert receipt['status'] == 'PASS_REAL_NATIVE_EDITOR' and receipt['host'] == 'codex'
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
    product = pathlib.Path(env['CARGO_TARGET_DIR']) / 'debug/akasha-otsuge'
    if product.exists():
        r['binary_sha256'] = sha(product)
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
    r['native_config_unchanged'] = before == sha(N / 'config.toml')
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
    assert not alive and r['native_config_unchanged']
