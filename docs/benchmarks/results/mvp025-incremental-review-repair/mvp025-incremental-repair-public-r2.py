import datetime, hashlib, json, os, pathlib, shutil, subprocess, tempfile, time

E = pathlib.Path(__file__).parent
B = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / 'debug/akasha-otsuge'
sha = lambda data: hashlib.sha256(data).hexdigest()
print(json.dumps({'binary': str(B), 'binary_sha256': sha(B.read_bytes()), 'utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}), flush=True)
children = []
calls = []
with tempfile.TemporaryDirectory(prefix='a25-repair-', dir=os.environ['TMPDIR']) as temporary:
    S = pathlib.Path(temporary)
    assert S.stat().st_dev == pathlib.Path(os.environ['TMPDIR']).stat().st_dev
    source = (E / 'mvp025-r1-retained-probe2.py').read_text()
    namespace = {'S': S / 'attempt2', 'json': json, 'sha': sha}
    namespace['S'].mkdir(mode=0o700)
    # Retain the review's exact independent archive constructor.
    exec(source[source.index('def archive('):source.index('def wait(')], namespace)
    old = namespace['archive']('old', 1)
    new = namespace['archive']('new', 2)
    leaf = (E / 'mvp025-r1-retained-leaf_probe.py').read_text()
    original_assert = "assert p.returncode==0 and not (data/'.archive-import').exists() and not (data/'.archive-replace.json').exists()"
    assert leaf.count(original_assert) == 1
    leaf = leaf.replace(original_assert, "assert p.returncode!=0 and (data/'.archive-import/victim').read_bytes()==b'foreign data must survive' and (data/'.archive-replace.json').is_file()")
    leaf = leaf.replace("print('CONFIRMED new cleanup deletes substituted foreign contents and reports success')", "print('CONFIRMED cleanup refused and foreign contents preserved')")
    leaf += '''
(data/'.archive-import').rename(r/'returned-foreign')
(r/'parked-old').rename(data/'.archive-import')
assert subprocess.run(cmd(S/'attempt2/new'),capture_output=True,timeout=20).returncode==0
exported=r/'exported'
assert subprocess.run([str(B),'--config',str(c),'archive','export',str(exported)],capture_output=True,timeout=20).returncode==0
for name in ['manifest.json','commits.jsonl','blobs.bin']:
 if name=='manifest.json':
  assert json.loads((exported/name).read_bytes())==json.loads((S/'attempt2/new'/name).read_bytes())
 else:
  assert (exported/name).read_bytes()==(S/'attempt2/new'/name).read_bytes()
assert (r/'returned-foreign/victim').read_bytes()==b'foreign data must survive'
assert not (data/'.archive-replace.json').exists()
print('CLEANUP_RECOVERY commit/blob bytes exact and all manifest fields equal; foreign victim retained')
'''
    (S / 'leaf.py').write_text(leaf)
    print('LEAF_SOURCE_SHA256', sha(leaf.encode()), flush=True)
    subprocess.run(['/usr/bin/python3', str(S / 'leaf.py')], check=True, timeout=60)

    fault = (E / 'mvp025-r1-retained-fault.c').read_text()
    old_hook = ' if(wanted && !strcmp(p,wanted))pause_probe();'
    assert fault.count(old_hook) == 1
    fault = fault.replace(old_hook, '''
 const char *directory=getenv("PROBE_DATA");
 const char *slash=strrchr(p,'/');
 if(wanted && directory && slash && !strcmp(slash+1,"authority")) {
  char *parent=strndup(p,(size_t)(slash-p));struct stat actual,expected;
  if(parent && !stat(parent,&actual) && !stat(directory,&expected)
     && actual.st_dev==expected.st_dev && actual.st_ino==expected.st_ino)pause_probe();
  free(parent);
 }
''')
    (S / 'fault.c').write_text(fault)
    print('FAULT_SOURCE_SHA256', sha(fault.encode()), flush=True)
    subprocess.run(['cc', '-shared', '-fPIC', '-Wall', '-Wextra', '-Werror', str(S/'fault.c'), '-ldl', '-o', str(S/'fault.so')], check=True, timeout=30)

    def config(name, socket):
        root = S/name
        root.mkdir(mode=0o700)
        (root/'data').mkdir(mode=0o700)
        path = root/'config.toml'
        path.write_text('[paths]\ndata_dir='+json.dumps(str(root/'data'))+'\nsocket_path='+json.dumps(str(S/socket))+'\n')
        return root, path

    def command(config, *args):
        return [str(B), '--config', str(config), *map(str,args)]

    def cli(config, *args, env=None):
        result = subprocess.run(command(config,*args),env=env,capture_output=True,timeout=20)
        error = json.loads(result.stderr)['error']['code'] if result.stderr else None
        calls.append({'args':list(map(str,args)), 'exit':result.returncode, 'error':error})
        print('CLI_RESULT',json.dumps(calls[-1]),flush=True)
        return result

    def launch(config, env=None):
        process = subprocess.Popen(command(config,'daemon','run'),env=env,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        children.append(process)
        return process

    try:
        root,c = config('durability','s')
        assert cli(c,'archive','import',old).returncode == 0
        data=root/'data';old_inode=(data/'authority').stat().st_ino
        env=os.environ.copy()
        env.update(LD_PRELOAD=str(S/'fault.so'),PROBE_MODE='eio',PROBE_DATA=str(data),PROBE_AUTH=str(data/'authority'),PROBE_OLD_INO=str(old_inode),PROBE_REACHED=str(root/'faults'))
        result=cli(c,'archive','import',new,env=env)
        assert result.returncode!=0 and json.loads(result.stderr)['error']['code']=='STORAGE_IO'
        assert (root/'faults').read_bytes()==b'x' and (data/'.archive-replace.json').is_file()
        for args in [('archive','export',root/'denied-export'),('daemon','run')]:
            result=cli(c,*args,env=env)
            assert result.returncode!=0 and json.loads(result.stderr)['error']['code']=='STORAGE_IO'
        assert (root/'faults').read_bytes()==b'xxx' and not (root/'denied-export').exists()
        print('DURABILITY_NEGATIVE both public opens attempted sync and refused EIO',flush=True)

        daemon=launch(c)
        deadline=time.monotonic()+15
        while True:
            result=subprocess.run(command(c,'daemon','status'),capture_output=True,timeout=2)
            if result.returncode==0:break
            assert daemon.poll() is None and time.monotonic()<deadline
            time.sleep(.01)
        assert cli(c,'daemon','stop').returncode==0 and daemon.wait(timeout=20)==0
        result=cli(c,'archive','export',root/'before-replay')
        assert result.returncode==0 and json.loads(result.stdout)['data']['commit_seq']>2
        assert (data/'.archive-replace.json').is_file()
        assert cli(c,'archive','import',new).returncode==0
        assert cli(c,'archive','export',root/'after-replay').returncode==0
        for name in ['manifest.json','commits.jsonl','blobs.bin']:
            assert (root/'before-replay'/name).read_bytes()==(root/'after-replay'/name).read_bytes()
        assert not (data/'.archive-replace.json').exists()
        print('DURABILITY_RECOVERY successful ordinary sync permits local commits; validated replay preserves full local tail',flush=True)

        root,c=config('open-swap','t')
        foreign=root/'foreign';foreign.mkdir(mode=0o700);(foreign/'keep').write_bytes(b'keep')
        env=os.environ.copy()
        env.update(LD_PRELOAD=str(S/'fault.so'),PROBE_DATA=str(root/'data'),PROBE_MKDIR='authority',PROBE_REACHED=str(root/'reached'),PROBE_RELEASE=str(root/'release'))
        daemon=launch(c,env)
        deadline=time.monotonic()+15
        while not (root/'reached').exists():
            assert daemon.poll() is None and time.monotonic()<deadline
            time.sleep(.005)
        (root/'data').rename(root/'parked');foreign.rename(root/'data');(root/'release').touch()
        assert daemon.wait(timeout=20)!=0
        assert (root/'data/keep').read_bytes()==b'keep'
        assert not (root/'data/authority').exists()
        assert sorted(p.name for p in (root/'data').iterdir())==['keep']
        print('ROOT_SUBSTITUTION refused; foreign directory byte content unchanged',flush=True)
    finally:
        for child in children:
            if child.poll() is None:
                child.kill();child.wait(timeout=20)
        print('OWNED_DAEMON_CHILDREN_REAPED',len(children),all(child.poll() is not None for child in children),flush=True)
        print('DISPOSABLE_SOURCE leaf.py\n'+leaf+'\nEND_DISPOSABLE_SOURCE',flush=True)
        print('DISPOSABLE_SOURCE fault.c\n'+fault+'\nEND_DISPOSABLE_SOURCE',flush=True)
print('EXACT_SCRATCH_REMOVED',not pathlib.Path(temporary).exists(),flush=True)
print('PUBLIC_REPAIR_ALL_CONTROLS_PASS',len(calls),flush=True)
