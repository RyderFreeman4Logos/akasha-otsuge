"""Real native Caller -> durable Consult -> native Runner; owned synthetic workspace."""
import datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, sys, tempfile, time
E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp016')
A = W / 'target/debug/akasha-otsuge'
PI = pathlib.Path('/home/obj/.local/share/mise/installs/npm-earendil-works-pi-coding-agent/1.4.0/node_modules/.bin/pi')
pin = json.loads((E/'mvp016-codex-pin.json').read_text())
CODEX = pathlib.Path(pin['binary'])
assert hashlib.sha256(CODEX.read_bytes()).hexdigest() == pin['binary_sha256']
assert pin['version'] == 'codex-cli 0.164.0'
TMP = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp')
case, tag = sys.argv[1:3]
assert case == 'hermes-empty'
HERMES = pathlib.Path('/home/obj/.local/bin/hermes')
head = os.environ['AKASHA_ACCEPTANCE_HEAD']
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip() == head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=W)
os.umask(0o077)
root = pathlib.Path(tempfile.mkdtemp(prefix='a16-native-',dir=TMP))
workspace, profile = root/'workspace', root/'profiles/acceptance'
workspace.mkdir(); profile.mkdir(parents=True)
(root/'installs').symlink_to('/home/obj/.hermes/installs', target_is_directory=True)
(workspace/'decision.md').write_text('Offline search policy: if embeddings are unavailable, keep keyword search working. Never silently send the query to another provider. This decision applies to this disposable acceptance workspace.\n')
env = dict(os.environ,PATH='/home/obj/.hermes/node/bin:/usr/bin:/bin',TMPDIR=str(root),TMP=str(root),TEMP=str(root),PYTHONDONTWRITEBYTECODE='1')
for key in ['AKASHA_EXECUTOR_MODEL','AKASHA_EXECUTOR_KEY','AKASHA_PI_RUN_SOCKET','AKASHA_PI_RUN_TOKEN']:env.pop(key,None)
native = pathlib.Path('/home/obj/.pi/agent')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
before = {name:sha(native/name) for name in ['models.json','settings.json']}
children=[]; logs=[]; started=time.monotonic()
r={'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'case':case,'head':head,'tree':subprocess.check_output(['git','rev-parse','HEAD^{tree}'],cwd=W,text=True).strip(),'binary_sha256':sha(A),'scratch':str(root),'status':'FAILED','checks':[],'provider_origin':'real authenticated native model; no synthetic provider responses'}
output=E/('mvp016-native-'+tag+'.json')
assert not output.exists()
def publish(phase):
 r.update(phase=phase,elapsed_seconds=round(time.monotonic()-started,3))
 output.write_text(json.dumps(r,indent=2)+'\n')
 print(json.dumps({'phase':phase,'case':case,'elapsed_seconds':r['elapsed_seconds']}),flush=True)
config=root/'akasha.toml';sock=root/'service.sock'
base=[str(A),'--config',str(config),'--socket',str(sock)]
def cli(*args):
 result=subprocess.run([*base,*args],env=env,cwd=workspace,capture_output=True,timeout=30)
 if result.returncode:
  try:error=json.loads(result.stderr)
  except (ValueError,UnicodeDecodeError):error={'stderr_sha256':hashlib.sha256(result.stderr).hexdigest()}
  raise AssertionError(('cli',args,result.returncode,error))
 return json.loads(result.stdout)['data']
def launch(args,name):
 out=(root/(name+'.out')).open('wb');err=(root/(name+'.err')).open('wb');logs.extend([out,err])
 p=subprocess.Popen(args,env=env,cwd=workspace,stdin=subprocess.DEVNULL,stdout=out,stderr=err,start_new_session=True)
 children.append(p);return p
try:
 settings='[paths]\ndata_dir='+json.dumps(str(root/'data'))+'\nsocket_path='+json.dumps(str(sock))+'\n[access]\nread_roots=['+json.dumps(str(root))+','+json.dumps('/home/obj/.codex')+']\n'
 if case=='pi-delegate':settings+='[runtime]\nmax_concurrent_jobs=2\n'
 if case=='pi-codex':settings+='[consult]\nexecutor="native_codex"\n[executors.native_codex]\nkind="codex"\nbinary='+json.dumps(str(CODEX))+'\nnative_home="/home/obj/.codex"\n'
 config.write_text(settings)
 if case.startswith('pi'):
  provider=json.loads((native/'models.json').read_text())['providers']['mp-cpi']
  assert provider['apiKey'].startswith('!')
  provider['baseUrl']='http://127.0.0.1:48317/v1'
  provider['models']=[m for m in provider['models'] if m['id']=='grok-4.7'];assert len(provider['models'])==1
  (profile/'models.json').write_text(json.dumps({'providers':{'mp-cpi':provider}}))
  (profile/'settings.json').write_text(json.dumps({'defaultProvider':'mp-cpi','defaultModel':'grok-4.7','defaultThinkingLevel':'off'}))
  env['PI_CODING_AGENT_DIR']=str(profile)
 else:
  provider=json.loads((native/'models.json').read_text())['providers']['mp-cpi']
  assert provider['apiKey'].startswith('!')
  resolved=subprocess.run(provider['apiKey'][1:],shell=True,env=env,capture_output=True,timeout=15)
  assert resolved.returncode==0 and 0<len(resolved.stdout.strip())<16384,'existing CPI credential resolver'
  env['OPENAI_API_KEY']=resolved.stdout.decode().strip()
  env['OPENAI_BASE_URL']='http://127.0.0.1:48317/v1'
  env['HERMES_HOME']=str(profile)
  env['HERMES_DISABLE_LAZY_INSTALLS']='1'
  cfg={'updates':{'check':False},'model':{'default':'grok-4.7','provider':'custom','base_url':env['OPENAI_BASE_URL'],'api_mode':'chat_completions','key_env':'OPENAI_API_KEY'},'plugins':{'enabled':['akasha-otsuge'],'isolation':'in_process'},'tools':{'tool_search':{'enabled':'off'}},'agent':{'max_turns':12,'run_budget_seconds':300},'terminal':{'cwd':str(workspace)},'gateway':{'standalone':True,'auto_multiplex_migration':False},'sessions':{'max_export_messages':128}}
  (profile/'config.yaml').write_text(json.dumps(cfg))

 daemon=launch([*base,'daemon','run'],'daemon')
 end=time.monotonic()+30
 while not sock.exists():
  assert daemon.poll() is None and time.monotonic()<end,'daemon readiness'
  time.sleep(.05)
 cli('daemon','status')
 if os.environ.get('AKASHA_NATIVE_INIT_INDEX')=='1':
  accepted=cli('index','sync','--idempotency-key','native-empty-index')
  end=time.monotonic()+30
  while True:
   indexed=cli('job','show',accepted['id'])
   if indexed['state'] not in ('queued','running'):break
   assert time.monotonic()<end,'empty index initialization'
   time.sleep(.1)
  assert indexed['state']=='succeeded',('index initialization',indexed)

 prompt='In this disposable workspace, what should the project do when embeddings are offline? Use Akasha Consult exactly once to research the current question and decision.md. Do not answer the policy yourself. After the tool returns, report only its job status. '
 if case.startswith('pi'):
  cli('adapter','install','pi','--binary',str(PI),'--agent-dir',str(profile))
  if case=='pi-delegate':prompt+='Call akasha_consult with hint "Delegate one bounded subquestion about offline search, then combine the evidence."'
  else:prompt+='Call akasha_consult with no hint (empty arguments).'
  args=[str(PI),'--mode','json','--print','--session',str(root/'caller.jsonl'),'--no-builtin-tools','--no-extensions','--no-mcp','--no-skills','--no-prompt-templates','--no-themes','--no-context-files','--no-approve','-e',str(profile/'extensions/akasha-otsuge/index.ts'),'--tools','akasha_consult',prompt]
 else:
  cli('adapter','install','hermes','--binary',str(HERMES),'--home',str(profile))
  gateway=launch([str(HERMES),'gateway','run','--quiet','--no-supervise','--external-supervisor'],'gateway')
  end=time.monotonic()+90
  while True:
   assert gateway.poll() is None,'gateway early exit'
   try:state=json.loads((profile/'gateway_state.json').read_text())
   except (FileNotFoundError,ValueError):state={}
   if state.get('pid')==gateway.pid and state.get('gateway_state') in ('running','degraded'):break
   assert time.monotonic()<end,'gateway readiness'
   time.sleep(.1)
  prompt+='Call akasha_consult with no hint (empty arguments).'
  request=root/'request.txt';request.write_text(prompt)
  args=[str(HERMES),'chat','--oneshot','--query-file',str(request),'--format','stream-json','--toolsets','akasha','--ignore-rules','--in',str(workspace)]

 r['native_binary']=str(HERMES)
 r['native_version']=subprocess.check_output([r['native_binary'],'--version'],env=env,text=True,timeout=15).strip()
 publish('native_caller_start')
 caller=launch(args,'caller')
 caller.wait(timeout=240)
 r['caller_exit']=caller.returncode
 r['caller_output_sha256']=sha(root/'caller.out')
 r['caller_error_sha256']=sha(root/'caller.err')
 assert caller.returncode==0,('native caller exit',caller.returncode)
 jobs=cli('job','list')['items'];consults=[j for j in jobs if j['id'].startswith('consult/')]
 r['consult_jobs']=len(consults);assert len(consults)==1,('trusted native submission',len(consults))
 job_id=consults[0]['id'];r['job_id']=job_id;publish('native_caller_submitted')
 end=time.monotonic()+240
 while True:
  job=cli('job','show',job_id)
  if job['state'] not in ('queued','running'):break
  assert time.monotonic()<end,'consult completion'
  time.sleep(.5)
 r['job']=job
 public=cli('consult','show',job_id);r['public']=public;publish('consult_terminal')
 assert job['state']=='succeeded',('consult failed',job.get('error'))
 assert public['status'] in ('found','incomplete'),public['status']
 assert any(code['path']=='decision.md' and code['sha256']==sha(workspace/'decision.md') for code in public['code']),'missing actual policy source'
 assert public['answer'].strip(),'empty researched answer'
 receipt=cli('consult','receipt',job_id)
 assert json.loads(receipt['data']['output'])==public,'public/receipt mismatch'
 r['receipt_id']=public['receipt_id']
 assert len(json.dumps(public,ensure_ascii=False))<12000
 runs=job['run_trace']['runs'];assert runs
 assert all(run['host']=='hermes' for run in runs)
 if case=='pi-delegate':
  assert len(runs)>=2 and any(run['parent'] is not None for run in runs),'real native delegate missing'
 r['checks']=['real native caller submitted exactly once','default-self or explicitly named runner','public terminal conclusion','durable receipt equals public output','shared run trace']
 if case=='pi-delegate':r['checks'].append('real native descendant delegation')
 cli('daemon','stop');daemon.wait(timeout=30);assert daemon.returncode==0
 r['status']='PASS_REAL_NATIVE_CONSULT'
except BaseException as exc:
 r['failure']=repr(exc)[:1500]
 raise
finally:
 for child in reversed(children):
  if child.poll() is None:
   child.send_signal(signal.SIGTERM)
   try:child.wait(timeout=30)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 for log in logs:log.close()
 alive=[]
 for proc in pathlib.Path('/proc').iterdir():
  if not proc.name.isdecimal():continue
  try:
   cwd=(proc/'cwd').resolve();state=(proc/'stat').read_text().rsplit(') ',1)[1].split()[0]
   if state!='Z' and (cwd==root or root in cwd.parents):alive.append(int(proc.name))
  except OSError:pass
 r['owned_live_processes']=alive
 r['native_settings_unchanged']=before=={name:sha(native/name) for name in before}
 r['head_unchanged']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip()==head
 r['allocated_bytes']=sum(p.lstat().st_blocks*512 for p in root.rglob('*') if p.is_file() and not p.is_symlink())
 if not alive:
  assert root.parent==TMP and not root.is_symlink()
  # Preserve only bounded synthetic failure diagnostics, never profiles or credentials.
  if r['status']!='PASS_REAL_NATIVE_CONSULT':
   for name in ['caller.out','caller.err','daemon.err','gateway.err']:
    if (root/name).exists():(E/('mvp016-native-'+tag+'-'+name)).write_bytes((root/name).read_bytes()[-262144:])
  shutil.rmtree(root)
 r['scratch_removed']=not root.exists();r['ssd_free_bytes_after']=shutil.disk_usage(TMP).free
 r['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 publish('finished')
 assert not alive and r['native_settings_unchanged'] and r['head_unchanged']
