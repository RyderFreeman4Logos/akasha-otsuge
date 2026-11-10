import contextlib,datetime,fcntl,hashlib,json,os,pathlib,sqlite3,subprocess,tempfile
E=pathlib.Path(__file__).parent;W=pathlib.Path.cwd();baseline=W/'target/release/akasha-otsuge';candidate=W/'target/debug/akasha-otsuge'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(baseline)=='35a2e2eab97b753dd4d200e7d27e45dffd996f2627c7a865e96917f706ec4bc2'
observations=[]
with tempfile.TemporaryDirectory(prefix='mvp025-directory-probe-',dir=os.environ['TMPDIR']) as temporary:
 R=pathlib.Path(temporary);lib=R/'probe.so';subprocess.run(['cc','-shared','-fPIC','-Wall','-Wextra','-Werror','-o',str(lib),str(E/'mvp025-mempal-directory-probe.c'),'-ldl'],check=True)
 def fixture(c):
  c.mkdir(mode=0o700);s=c/'source.db';db=sqlite3.connect(s);db.executescript((W/'tests/fixtures/mempal21-28.sql').read_text());db.execute("INSERT INTO drawers(id,content,wing,added_at,project_id) VALUES('test',?,'test','2026-01-01 00:00:00','project')",('original\0原文'*200,));db.commit();db.close();s.chmod(0o600)
  cfg=c/'config.toml';cfg.write_text('[paths]\ndata_dir='+json.dumps(str(c/'data'))+'\nsocket_path='+json.dumps(str(c/'socket'))+'\n');return s,cfg
 def command(binary,cfg,s,a,env=None):return subprocess.run([str(binary),'--config',str(cfg),'migrate','mempal',str(s),str(a)],env=env,capture_output=True,timeout=60)
 c=R/'compatibility';s,cfg=fixture(c);a=c/'archive';old=command(baseline,cfg,s,a);assert old.returncode==0
 names=['conversion.json','manifest.json','commits.jsonl','blobs.bin'];before={n:(a/n).read_bytes() for n in names};new=command(candidate,cfg,s,a)
 assert new.returncode==0 and new.stdout==old.stdout and before=={n:(a/n).read_bytes() for n in names}
 observations.append({'phase':'baseline_archive_candidate_replay','exit':0,'all_four_files_unchanged':True})
 for label,binary in [('baseline',baseline),('candidate',candidate)]:
  for phase in ['identity-directory','manifest-directory','identity-locked-directory','manifest-locked-directory']:
   c=R/(label+'-'+phase);s,cfg=fixture(c);a=c/'archive';park=c/'parked';source_bytes=s.read_bytes();identity=(s.stat().st_dev,s.stat().st_ino)
   env=os.environ.copy();env.update(LD_PRELOAD=str(lib),R4_ROOT=str(a),R4_PARK=str(park),R4_PREFIX='{"mapping_version":' if phase.startswith('identity') else '{"archive_version":')
   with contextlib.ExitStack() as stack:
    held=None
    if 'locked' in phase:
     other=c/'replacement';other.mkdir(mode=0o700);lock=other/'.mempal-convert.lock';lock.touch(mode=0o600);held=stack.enter_context(lock.open('r+b'));fcntl.flock(held,fcntl.LOCK_EX|fcntl.LOCK_NB);env['R4_OTHER']=str(other)
    result=command(binary,cfg,s,a,env);observed=b'R4_MOVE_OBSERVED' in result.stderr
    preserved=s.read_bytes()==source_bytes and (s.stat().st_dev,s.stat().st_ino)==identity
    members=sorted(x.name for x in a.iterdir());expected=['.mempal-convert.lock'] if held else []
    row={'binary':label,'phase':phase,'exit':result.returncode,'move_observed':observed,'source_preserved':preserved,'replacement_untouched':members==expected}
    assert observed and preserved
    if held:
     assert os.fstat(held.fileno()).st_ino==(a/'.mempal-convert.lock').stat().st_ino
     busy=command(binary,cfg,s,a);assert busy.returncode!=0 and json.loads(busy.stderr)['error']['code']=='MEMPAL_CONVERSION_BUSY';row['independent_lock_busy_control']=True
    if label=='baseline':assert result.returncode==0
    else:assert result.returncode!=0 and members==expected
   if label=='candidate':
    resumed=command(candidate,cfg,s,park);assert resumed.returncode==0
    saved={n:(park/n).read_bytes() for n in names};repeat=command(candidate,cfg,s,park)
    assert repeat.returncode==0 and repeat.stdout==resumed.stdout and saved=={n:(park/n).read_bytes() for n in names}
    for args in [('archive','import',str(park)),('archive','export',str(c/'export'))]:
     p=subprocess.run([str(candidate),'--config',str(cfg),*args],capture_output=True,timeout=60);assert p.returncode==0,p.stderr
    assert all((c/'export'/n).read_bytes()==saved[n] for n in names[1:]);row['parked_recovery_repeat_roundtrip']=True
   observations.append(row);print(json.dumps(row),flush=True)
r={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'PASS','baseline_binary_sha256':sha(baseline),'candidate_binary_sha256':sha(candidate),'candidate_writer_sha256':sha(W/'src/storage/mempal/writer.rs'),'interposer_sha256':sha(E/'mvp025-mempal-directory-probe.c'),'observations':observations,'owned_fixture_removed':not R.exists(),'limits':['Tiny public structural fixtures; no full private data or live hosts.','The controlled actor renames the directory; the converter must preserve output ownership and refuse false success.']}
with (E/'mvp025-mempal-directory-probe-r1.json').open('x') as f:json.dump(r,f,indent=2)
print(json.dumps({'status':r['status'],'observations':len(observations),'owned_fixture_removed':r['owned_fixture_removed']}),flush=True)
