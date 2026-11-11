import datetime, errno, hashlib, json, os, pathlib, select, subprocess, sys, time

E=pathlib.Path(__file__).parent
def emit(value):print(json.dumps(value),flush=True)
def group():
    rows=[s[3:] for s in pathlib.Path('/proc/self/cgroup').read_text().splitlines() if s.startswith('0::')]
    assert len(rows)==1
    return pathlib.Path('/sys/fs/cgroup')/rows[0].lstrip('/')
def ready(pipe, seconds=30):
    assert select.select([pipe],[],[],seconds)[0], 'bounded fixture readiness timeout'
    line=pipe.readline();assert line, 'fixture exited before readiness';return json.loads(line)
def memory(path):return {n:(path/n).read_text().strip() for n in ['memory.current','memory.peak','memory.high','memory.max','memory.events','cgroup.procs','cgroup.events']}

if len(sys.argv)>1 and sys.argv[1]=='grandchild':
    payload=bytearray(4*1024*1024)
    emit({'pid':os.getpid(),'group':str(group())})
    sys.stdin.readline()
    raise SystemExit(0)
if len(sys.argv)>1 and sys.argv[1]=='worker':
    runner=pathlib.Path(sys.argv[2]);(runner/'cgroup.procs').write_text(str(os.getpid()))
    payload=bytearray(4*1024*1024)
    child=subprocess.Popen([sys.executable,__file__,'grandchild'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    try:
        nested=ready(child.stdout);emit({'pid':os.getpid(),'group':str(group()),'grandchild':nested})
        sys.stdin.readline()
    finally:
        child.communicate('finish\n',timeout=10)
        assert child.returncode==0
    raise SystemExit(0)
if len(sys.argv)>1 and sys.argv[1]=='owner':
    unit=sys.argv[2]
    argv=['/usr/bin/busctl','--user','--timeout=10','call','org.freedesktop.systemd1','/org/freedesktop/systemd1','org.freedesktop.systemd1.Manager','StartTransientUnit','ssa(sv)a(sa(sv))',unit,'fail','4','PIDs','au','1',str(os.getpid()),'Delegate','b','true','MemoryAccounting','b','true','CollectMode','s','inactive-or-failed','0']
    result=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,timeout=12)
    assert result.returncode==0,(result.returncode,result.stderr)
    root=group()
    initial={'path':str(root),'owner_uid':root.stat().st_uid,'uid':os.getuid()}
    print(json.dumps({'stage':'initial_membership','unit':unit,**initial}),file=sys.stderr,flush=True)
    end=time.monotonic()+10
    while root.name!=unit or root.stat().st_uid!=os.getuid():
        assert time.monotonic()<end, {'unit':unit,'path':str(root),'owner_uid':root.stat().st_uid,'uid':os.getuid()}
        time.sleep(0.01);root=group()
    emit({'stage':'scope_created','pid':os.getpid(),'root':str(root),'command':argv})
    if sys.stdin.readline()!='go\n':raise SystemExit(0)
    daemon=root/'daemon';runner=root/'runner';daemon.mkdir();runner.mkdir()
    (daemon/'cgroup.procs').write_text(str(os.getpid()));assert not (root/'cgroup.procs').read_text().strip()
    (root/'cgroup.subtree_control').write_text('+memory')
    limits={root:(96,128),daemon:(48,64),runner:(32,64)}
    for path,(high,maximum) in limits.items():
        (path/'memory.high').write_text(str(high*1024*1024));(path/'memory.max').write_text(str(maximum*1024*1024))
        assert int((path/'memory.high').read_text())==high*1024*1024 and int((path/'memory.max').read_text())==maximum*1024*1024
    (root/'memory.swap.max').write_text('0')
    child=subprocess.Popen([sys.executable,__file__,'worker',str(runner)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    try:
        worker=ready(child.stdout)
        assert worker['group']==worker['grandchild']['group']==str(runner)
        assert set(map(int,(runner/'cgroup.procs').read_text().split()))=={worker['pid'],worker['grandchild']['pid']}
        sampled={path.name:memory(path) for path in limits}
        assert int(sampled['runner']['memory.current'])>=8*1024*1024
    finally:
        child.communicate('finish\n',timeout=10);assert child.returncode==0
    after=memory(runner);assert not after['cgroup.procs'].strip() and 'populated 0' in after['cgroup.events']
    assert all('oom_kill 0' in row['memory.events'] for row in sampled.values())
    runner.rmdir()
    emit({'stage':'passed','owner_pid':os.getpid(),'owner_group':str(group()),'worker':worker,'samples':sampled,'runner_empty_after_reap':after,'scope_root':str(root),'limits_mib':{'service':[96,128],'daemon':[48,64],'runner':[32,64]}})
    raise SystemExit(0)

unit=f'akasha-otsuge-capability-{os.getpid()}-{time.time_ns()}.scope'
child=subprocess.Popen([sys.executable,__file__,'owner',unit],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
emit({'stage':'fixture_dispatch','unit':unit,'owned_pid':child.pid})
events=kill=None;created=None;receipt=None
try:
    created=ready(child.stdout)
    assert created['stage']=='scope_created' and created['pid']==child.pid
    root=pathlib.Path(created['root']);assert root.name==unit and root.stat().st_uid==os.getuid()
    assert set(map(int,(root/'cgroup.procs').read_text().split()))=={child.pid}
    events=os.open(root/'cgroup.events',os.O_RDONLY|os.O_CLOEXEC);kill=os.open(root/'cgroup.kill',os.O_WRONLY|os.O_CLOEXEC)
    emit({'stage':'parent_bound_scope','unit':unit,'root':str(root),'owned_pid':child.pid})
    stdout,stderr=child.communicate('go\n',timeout=30);assert child.returncode==0,(child.returncode,stdout,stderr)
    receipt=json.loads(stdout);assert receipt['stage']=='passed'
finally:
    if child.poll() is None or receipt is None:
        if kill is not None:
            try:os.write(kill,b'1')
            except OSError as error:
                if error.errno!=errno.ENODEV:raise
        elif child.poll() is None:child.terminate()
        child.wait(timeout=10)
    if kill is not None:os.close(kill)
if events is not None:
    try:
        state=os.pread(events,4096,0).decode();assert 'populated 0' in state;fence='bound root populated 0 after owner reap'
    except OSError as error:
        assert error.errno==errno.ENODEV;fence='bound root collected (ENODEV) after owner reap'
    finally:os.close(events)
else:raise AssertionError('scope binding unavailable')
end=time.monotonic()+10
while root.exists() and time.monotonic()<end:time.sleep(0.05)
assert not root.exists(), 'owned empty unit not collected'
post=subprocess.run(['/usr/bin/systemctl','--user','show',unit,'--property=LoadState,ActiveState,SubState'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,timeout=10)
assert 'LoadState=not-found' in post.stdout and 'ActiveState=inactive' in post.stdout
r={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'script_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),'unit':unit,'creation':created,'functional':receipt,'root_fence':fence,'post_unit':post.stdout,'product_source_unchanged':True,'limit':'API and normal owned lifecycle capability only; no OOM, forced supervisor death, workload or product enforcement acceptance.'}
with (E/'mvp024-owned-scope-capability-r3.json').open('x') as f:json.dump(r,f,indent=2)
emit(r)
