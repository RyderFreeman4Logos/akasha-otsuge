"""Controlled real Pi wall-clock qualification; synthetic endpoint, no paid model."""
import datetime, hashlib, json, os, pathlib, shutil, signal, subprocess, sys, tempfile, threading, time
W=pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp016')
E=pathlib.Path(__file__).parent
sys.path.insert(0,str(W/'tests/fixtures/pi/1.4.0'))
from mvp016_native_provider_long import Provider
A=W/'target/debug/akasha-otsuge';PI=pathlib.Path('/home/obj/.local/share/mise/installs/npm-earendil-works-pi-coding-agent/1.4.0/node_modules/.bin/pi')
TEST=pathlib.Path(sys.argv[1]);OUT=E/sys.argv[2]
OUT.mkdir(mode=0o700);os.umask(0o077)
TMP=pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp')
assert shutil.disk_usage(TMP).free>128*1024*1024
root=pathlib.Path(tempfile.mkdtemp(prefix='a16-native-long-',dir=TMP));profile=root/'agent';profile.mkdir()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
start=time.monotonic();lock=threading.RLock();release=threading.Event();child=None;installed=False;rows=[]
state={'head':head,'tree':subprocess.check_output(['git','rev-parse','HEAD^{tree}'],cwd=W,text=True).strip(),'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scratch':str(root),'run_id':OUT.name,'kind':'actual Pi 1.4.0 RPC with controlled local SSE; no model quality or token cost claim','binaries':{str(p):sha(p) for p in [A,PI,TEST]},'harness_sha256':sha(pathlib.Path(__file__)),'controlled_provider_sha256':sha(E/'mvp016_native_provider_long.py'),'cases':rows,'active':None,'status':'FAILED'}
config='[execution]\nlong_delay_seconds = 1605\nrunner_deadline_seconds = 1700\nmax_tokens = 2048\n[reliability]\nmax_attempts = 1\nreason = "controlled deterministic protocol; no retry on semantic or timeout failure"\n'
(OUT/'config.toml').write_text(config);state['config_epoch']=hashlib.sha256(config.encode()).hexdigest()
def atomic(name,value):
 p=OUT/name;t=OUT/(name+'.tmp')
 with t.open('w') as f:json.dump(value,f,indent=2);f.write('\n');f.flush();os.fsync(f.fileno())
 os.replace(t,p);fd=os.open(OUT,os.O_DIRECTORY);os.fsync(fd);os.close(fd)
def checkpoint(phase,**fields):
 with lock:
  state.update(phase=phase,**fields);state['elapsed_seconds']=time.monotonic()-start
  atomic('state.json',state)
  atomic('progress.yaml',{'completed':len(rows),'total':3,'elapsed_s':state['elapsed_seconds'],'rate_per_s':len(rows)/state['elapsed_seconds'],'eta_s':None,'active_unit':state['active'],'pid':os.getpid(),'start_ticks':pathlib.Path('/proc/self/stat').read_text().rsplit(') ',1)[1].split()[19],'config_epoch':state['config_epoch'],'phase':phase})
checkpoint('admitted')
env=dict(os.environ,PATH='/home/obj/.hermes/node/bin:/usr/bin:/bin',TMPDIR=str(root),TMP=str(root),TEMP=str(root),AKASHA_PI_BINARY=str(PI),AKASHA_PI_HOME=str(profile),PYTHONDONTWRITEBYTECODE='1')
provider=Provider();original=provider.server.RequestHandlerClass.do_POST
pending={'delay':0,'seen':False};request_started=[]
def delayed(handler):
 with lock:
  delay=pending['delay'] if not pending['seen'] else 0;pending['seen']=True
  request_started.append(time.monotonic());checkpoint('provider_request',request_count=len(request_started),delay_seconds=delay)
 if delay and release.wait(delay):return
 if delay:checkpoint('provider_delay_finished',observed_delay_seconds=time.monotonic()-request_started[0])
 original(handler)
provider.server.RequestHandlerClass.do_POST=delayed

def cli(*args):
 p=subprocess.run([str(A),*args],cwd=root,env=env,capture_output=True,text=True,timeout=30)
 assert p.returncode==0,p.stderr[:1000]
def owned():
 result=[]
 for p in pathlib.Path('/proc').iterdir():
  if not p.name.isdecimal():continue
  try:
   cwd=pathlib.Path(os.readlink(p/'cwd').removesuffix(' (deleted)'));f=(p/'stat').read_text().rsplit(') ',1)[1].split()
   if (cwd==root or root in cwd.parents) and f[0]!='Z':result.append((int(p.name),f[19]))
  except (FileNotFoundError,ProcessLookupError,PermissionError):pass
 return result
try:
 provider.configure(profile)
 settings=json.loads((profile/'settings.json').read_text());settings['retry']={'enabled':False,'maxRetries':0,'provider':{'maxRetries':0}};(profile/'settings.json').write_text(json.dumps(settings))
 cli('--socket',str(root/'unused.sock'),'adapter','install','pi','--binary',str(PI),'--agent-dir',str(profile));installed=True
 for case,delay,deadline in [('protocol-control',0,180),('long-complete',1605,1700),('deadline-cleanup',25,10)]:
  assert hashlib.sha256((OUT/'config.toml').read_bytes()).hexdigest()==state['config_epoch'],'qualification config changed'
  provider.case='roundtrip';provider.requests=[];provider.errors=[];pending.update(delay=delay,seen=False);request_started.clear()
  checkpoint('unit_admitted',active=case,unit_started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
  t=time.monotonic();env['AKASHA_PI_ACCEPTANCE_SECONDS']=str(deadline)
  with (OUT/(case+'.log')).open('xb') as log:
   child=subprocess.Popen([str(TEST),'runner::installed_native_runner_roundtrip','--ignored','--exact','--nocapture'],cwd=root,env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
   try:code=child.wait(timeout=deadline+45)
   except subprocess.TimeoutExpired:code=124;os.killpg(child.pid,signal.SIGTERM);child.wait(timeout=30)
  elapsed=time.monotonic()-t;output=(OUT/(case+'.log')).read_text();residue=owned()
  ok=(code==0 and len(provider.requests)==5 and not provider.errors) if case!='deadline-cleanup' else (code==101 and 'RunnerTimeout' in output and '1 failed' in output and 9<=elapsed<30)
  if case=='long-complete':ok=ok and 1600<=elapsed<1700
  row={'case':case,'exit_code':code,'elapsed_seconds':elapsed,'deadline_seconds':deadline,'controlled_delay_seconds':delay,'request_count':len(request_started),'served_requests':provider.requests[:],'provider_errors':provider.errors[:],'owned_processes_after':residue,'passed':ok and not residue,'log_sha256':sha(OUT/(case+'.log'))}
  rows.append(row);checkpoint('unit_finished',active=None)
  if case=='protocol-control':assert row['passed'],row
 state['status']='PASS_REAL_NATIVE_LONG_AND_TIMEOUT' if all(r['passed'] for r in rows) else 'FAILED'
finally:
 release.set()
 if child is not None and child.poll() is None:os.killpg(child.pid,signal.SIGTERM);child.wait(timeout=30)
 for pid,ticks in owned():
  try:
   fd=os.pidfd_open(pid)
   try:
    if pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()[19]==ticks:signal.pidfd_send_signal(fd,signal.SIGKILL)
   finally:os.close(fd)
  except (FileNotFoundError,ProcessLookupError):pass
 end=time.monotonic()+5
 while owned() and time.monotonic()<end:time.sleep(.05)
 state['processes_ended']=not owned();assert state['processes_ended']
 if installed:cli('adapter','uninstall','pi','--agent-dir',str(profile))
 provider.close();assert root.parent==TMP and not root.is_symlink();shutil.rmtree(root)
 state['scratch_removed']=not root.exists();state['ssd_free_after']=shutil.disk_usage(TMP).free
 state['binaries_unchanged']={str(p):sha(p)==state['binaries'][str(p)] for p in [A,PI,TEST]}
 checkpoint('collected',finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 print(json.dumps({'status':state['status'],'cases':[{'case':r['case'],'passed':r['passed'],'seconds':r['elapsed_seconds']} for r in rows],'scratch_removed':state['scratch_removed']}),flush=True)
sys.exit(state['status']!='PASS_REAL_NATIVE_LONG_AND_TIMEOUT')
