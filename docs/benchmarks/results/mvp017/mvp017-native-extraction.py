import datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, tempfile, time, urllib.parse
E=pathlib.Path(__file__).parent
W=pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp017')
TMP=pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp')
PI=pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/npm-earendil-works-pi-coding-agent/1.4.0/node_modules/.bin/pi')
N=pathlib.Path('/home/obj/.pi/agent')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
tag=os.environ['AKASHA_NATIVE_TAG']; output=E/f'mvp017-native-{tag}.json'; result=E/f'mvp017-native-{tag}-result.json'
assert not output.exists() and not result.exists()
assert shutil.disk_usage(TMP).free>1024**3
os.umask(0o077); root=pathlib.Path(tempfile.mkdtemp(prefix='a17-native-',dir=TMP)); profile=root/'profile'; profile.mkdir()
before={n:sha(N/n) for n in ['models.json','settings.json']}
p=json.loads((N/'models.json').read_text())['providers']['mp-cpi']; p['baseUrl']='http://127.0.0.1:48317/v1'; u=urllib.parse.urlparse(p['baseUrl'])
assert u.hostname in ['localhost','127.0.0.1'] and u.port==48317
assert p['apiKey'].startswith('!')
p['models']=[m for m in p['models'] if m['id']=='grok-4.7'];assert len(p['models'])==1
(profile/'models.json').write_text(json.dumps({'providers':{'mp-cpi':p}}))
(profile/'settings.json').write_text(json.dumps({'defaultProvider':'mp-cpi','defaultModel':'grok-4.7','defaultThinkingLevel':'off'}))
env=dict(os.environ,TMPDIR=str(root),TMP=str(root),TEMP=str(root),AKASHA_NATIVE_PI_BINARY=str(PI),AKASHA_NATIVE_PI_HOME=str(profile),AKASHA_NATIVE_EXTRACTION_RECEIPT=str(result))
r={'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip(),'dirty_diff_sha256':hashlib.sha256(subprocess.check_output(['git','diff','--binary'],cwd=W)).hexdigest(),'source':'real authenticated Pi; existing native key-command reference; no Akasha model profiles','host_version':subprocess.check_output([str(PI),'--version'],env=env,text=True,timeout=15).strip(),'native_binary':str(PI.resolve()),'native_binary_sha256':sha(PI),'provider':'mp-cpi','model':'grok-4.7','endpoint_host':u.hostname,'endpoint_port':u.port,'scratch':str(root),'status':'FAILED'}
child=None; start=time.monotonic()
try:
 cmd=['cargo','test','--locked','--test','daemon','extraction_tests::native_single_cli_extraction_actual','--','--exact','--ignored','--nocapture']
 r['command']=cmd;output.write_text(json.dumps(r,indent=2)+'\n')
 child=subprocess.Popen(cmd,cwd=W,env=env,stdin=subprocess.DEVNULL,start_new_session=True)
 rc=child.wait(timeout=2400);r['exit_code']=rc
 assert rc==0 and result.exists()
 receipt=json.loads(result.read_text());assert receipt['status']=='PASS_REAL_SINGLE_CLI_EXTRACTION'
 r['binary_sha256']=sha(pathlib.Path(env['CARGO_TARGET_DIR'])/'debug/akasha-otsuge')
 r['result_sha256']=sha(result);r['candidate_count']=len(receipt['memories']);r['status']='PASS_REAL_SINGLE_CLI_EXTRACTION'
finally:
 if child and child.poll() is None:
  os.killpg(child.pid,signal.SIGTERM)
  try:child.wait(timeout=30)
  except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 alive=[]
 for proc in pathlib.Path('/proc').iterdir():
  if not proc.name.isdecimal():continue
  try:
   cwd=(proc/'cwd').resolve();state=(proc/'stat').read_text().rsplit(') ',1)[1].split()[0]
   if state!='Z' and (cwd==root or root in cwd.parents):alive.append(int(proc.name))
  except OSError:pass
 r['owned_live_processes']=alive;r['native_settings_unchanged']=before=={n:sha(N/n) for n in before}
 r['scratch_allocated_bytes']=sum(p.lstat().st_blocks*512 for p in root.rglob('*') if p.is_file() and not p.is_symlink())
 if not alive:
  assert root.parent==TMP and not root.is_symlink();shutil.rmtree(root)
 r['scratch_removed']=not root.exists();r['ssd_free_bytes_after']=shutil.disk_usage(TMP).free;r['seconds']=time.monotonic()-start;r['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
 assert not alive and r['native_settings_unchanged']
