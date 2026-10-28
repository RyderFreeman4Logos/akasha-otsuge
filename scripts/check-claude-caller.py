import datetime, fcntl, hashlib, http.server, json, os, pathlib, pty, re, select, shutil, signal, struct, subprocess, sys, termios, threading, time

# Uses the real host, hook module, daemon and Storage with local synthetic SSE.
# The scratch path must be new; all owned processes and scratch files are removed.
REPO=pathlib.Path(__file__).resolve().parents[1]
NATIVE, AKASHA, ROOT = [pathlib.Path(value).resolve() for value in sys.argv[1:]]
assert NATIVE.is_file() and AKASHA.is_file() and not ROOT.exists()
KEY='akasha-caller-fixture-not-real'
ROOT.mkdir(mode=0o700)
for name in ['home','config','workspace','plugin/.claude-plugin','plugin/hooks']:(ROOT/name).mkdir(parents=True,exist_ok=True)
socket_root=pathlib.Path('/tmp')/('akasha-caller-'+str(os.getpid()));socket_root.mkdir(mode=0o700)
socket=socket_root/'service.sock'
config=ROOT/'akasha.toml';config.write_text('[paths]\ndata_dir='+json.dumps(str(ROOT/'data'))+'\nsocket_path='+json.dumps(str(socket))+'\n[access]\nread_roots=['+json.dumps(str(ROOT))+']\n')
argv=[str(AKASHA),'--config',str(config),'--socket',str(socket)]
wrapper=ROOT/'bridge';wrapper.write_text('#!/usr/bin/python3\n'+'''import json,subprocess,sys,pathlib
event=json.load(sys.stdin)
result=subprocess.run(ARGV+['adapter','claude-call'],input=json.dumps(event).encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=12)
response=json.loads(result.stdout if result.returncode==0 else result.stderr)
record={'event':{k:v for k,v in event.items() if k not in ['registration','parent_registration']},'response':response}
if event['op'] in ['register','register_child'] and response.get('data'):record={'event':record['event'],'registered':True}
with LOG.open('a') as f:f.write(json.dumps(record)+'\\n')
sys.stdout.buffer.write(result.stdout);sys.stderr.buffer.write(result.stderr);sys.exit(result.returncode)
'''.replace('ARGV',repr(argv)).replace('LOG',"pathlib.Path("+repr(str(ROOT/'bridge.jsonl'))+")"));wrapper.chmod(0o700)
(ROOT/'plugin/.claude-plugin/plugin.json').write_text('{"name":"akasha-otsuge","version":"0.1.0","description":"Native caller protocol fixture"}')
(ROOT/'plugin/hooks/hooks.json').write_text('{"modules":["./entry.js"]}')
shutil.copyfile(REPO/'src/adapter/claude/caller.js',ROOT/'plugin/hooks/caller.js')
(ROOT/'plugin/hooks/config.js').write_text('export const bridge='+json.dumps(str(wrapper))+';\nexport const binary='+json.dumps(str(NATIVE))+';\nexport const args=[];\n')
(ROOT/'plugin/hooks/probe.js').write_text('''export function register(on) {
 on('session.start',async ($,e,next)=>{await $.tool.register({name:'consult',description:'Test native binding before a tool executes',inputSchema:{type:'object',properties:{hint:{type:'string'}},additionalProperties:false},isDeferred:false});return next(e);});
 on('tool.check',async ($,e,next)=>e.tool==='mcp__akasha-otsuge__consult'?{decision:'allow'}:next(e));
 on('tool.call',async ($,e,next)=>e.tool==='mcp__akasha-otsuge__consult'?{result:'native caller probe'}:next(e));
}''')
(ROOT/'plugin/hooks/entry.js').write_text("import {register as caller} from './caller.js'; import {register as probe} from './probe.js'; export function register(on) {caller(on);probe(on);}")
workspace=str((ROOT/'workspace').resolve())
(ROOT/'config/.claude.json').write_text(json.dumps({'hasCompletedOnboarding':True,'lastOnboardingVersion':'2.1.311','customApiKeyResponses':{'approved':[KEY[-20:]],'rejected':[]},'projects':{workspace:{'hasTrustDialogAccepted':True}}}))
stage = ''
count = 0
child_called = False
child_result_seen = False
requests = []
errors = []
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        global count, child_called, child_result_seen
        try:
            size=int(self.headers['Content-Length']);assert 0<size<1024*1024
            request=json.loads(self.rfile.read(size))
            if self.path.startswith('/v1/messages/count_tokens'):
                payload=b'{"input_tokens":20}';self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload);return
            assert self.path.startswith('/v1/messages') and self.headers.get('x-api-key')==KEY
            count+=1;assert count<=8
            title='Write the title in the predominant language' in json.dumps(request['messages'])
            requests.append({'stage':stage,'kind':'title' if title else 'conversation','body':request})
            (ROOT/'requests.json').write_text(json.dumps(requests))
            if stage=='fork':
                history=json.dumps(request['messages'])
                assert 'PRIVATE_CALLER_INPUT start' in history and 'PRIVATE_CALLER_INPUT resume' in history
                assert 'PRIVATE_CALLER_INPUT parent_later' not in history
            if stage=='parent_later':
                assert 'PRIVATE_CALLER_INPUT fork' not in json.dumps(request['messages'])
            is_child=stage=='child' and any(m.get('role')=='user' and (m.get('content')=='PRIVATE_CHILD_INPUT' or isinstance(m.get('content'),list) and any(b.get('text')=='PRIVATE_CHILD_INPUT' for b in m['content'])) for m in request['messages'])
            spawn=stage=='child' and count==1
            tool=stage not in ['installed','tui'] and (count==1 or is_child and not child_called)
            if is_child:
                if child_called:
                    assert 'native caller probe' in json.dumps(request['messages']), 'child consult body did not execute'
                    child_result_seen=True
                child_called=True
            block=dict(type='tool_use',id='toolu_'+stage+'_'+str(count),name='Agent' if spawn else 'mcp__akasha-otsuge__consult',input={}) if tool else dict(type='text',text='')
            delta=dict(type='input_json_delta',partial_json=json.dumps({'prompt':'PRIVATE_CHILD_INPUT','description':'child binding check','subagent_type':'general-purpose'} if spawn else {'hint':'probe'})) if tool else dict(type='text_delta',text='Fixture session title' if title else 'Observed caller '+stage)
            message=dict(id='msg_'+stage+'_'+str(count),type='message',role='assistant',model=request['model'],content=[],stop_reason=None,stop_sequence=None,usage=dict(input_tokens=20,output_tokens=1))
            events=[dict(type='message_start',message=message),dict(type='content_block_start',index=0,content_block=block),dict(type='content_block_delta',index=0,delta=delta),dict(type='content_block_stop',index=0),dict(type='message_delta',delta=dict(stop_reason='tool_use' if tool else 'end_turn',stop_sequence=None),usage=dict(output_tokens=20)),dict(type='message_stop')]
            payload=''.join('event: '+e['type']+'\ndata: '+json.dumps(e)+'\n\n' for e in events).encode()
            assert request['stream'] is True
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
        except Exception as error:
            errors.append(type(error).__name__);self.send_error(400)

server=http.server.HTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever);thread.start()
env=dict(HOME=str(ROOT/'home'),USER='obj',LOGNAME='obj',PATH='/usr/bin:/bin',LANG='C.UTF-8',CLAUDE_CONFIG_DIR=str(ROOT/'config'),DISABLE_AUTOUPDATER='1',CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1',CLAUDE_CODE_DISABLE_CLAUDE_MDS='1',ANTHROPIC_API_KEY=KEY,ANTHROPIC_BASE_URL='http://127.0.0.1:'+str(server.server_port))
def tui(args, expected):
 pid,master=pty.fork()
 if pid==0:
  os.chdir(workspace)
  fcntl.ioctl(0,termios.TIOCSWINSZ,struct.pack('HHHH',40,160,0,0))
  os.execve(str(NATIVE),args,dict(env,TERM='xterm-256color'))
 raw=bytearray();status=None;observed=False;tty=None;end=time.monotonic()+30
 try:
  while time.monotonic()<end:
   ready,_,_=select.select([master],[],[],.1)
   if ready:
    try:chunk=os.read(master,65536)
    except OSError:break
    if not chunk:break
    raw.extend(chunk);assert len(raw)<=2*1024*1024
    if tty is None:tty=pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()[4]
    clean=re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]','',raw.decode(errors='replace'))
    compact=re.sub(r'\s+','',clean)
    if not observed and expected.replace(' ','') in compact and re.search(r'done\d{1,2}:\d{2}',compact):
     observed=True;os.write(master,b'/exit\r')
   ended,candidate=os.waitpid(pid,os.WNOHANG)
   if ended:status=candidate;break
 finally:
  if status is None:
   ended,candidate=os.waitpid(pid,os.WNOHANG)
   if ended:status=candidate
   else:
    os.killpg(pid,signal.SIGTERM);end=time.monotonic()+5
    while time.monotonic()<end:
     ended,candidate=os.waitpid(pid,os.WNOHANG)
     if ended:status=candidate;break
     time.sleep(.05)
    if status is None:os.killpg(pid,signal.SIGKILL);_,status=os.waitpid(pid,0)
  os.close(master)
 assert observed and os.waitstatus_to_exitcode(status)==0 and int(tty or 0)!=0,raw[-4000:].decode(errors='replace')
 return {'stage':'tui','pid':pid,'tty_nr':tty,'requests':count,'stdout_sha256':hashlib.sha256(raw).hexdigest()}
daemon_log=(ROOT/'daemon.log').open('wb')
daemon=None
checks=[]
try:
 assert not any(shutil.which(host,path=env['PATH']) for host in ['codex','hermes','pi'])
 daemon=subprocess.Popen(argv+['daemon','run'],env=env,stdin=subprocess.DEVNULL,stdout=daemon_log,stderr=subprocess.STDOUT,start_new_session=True)
 validation=subprocess.run([str(NATIVE),'plugin','validate',str(ROOT/'plugin')],env=env,capture_output=True,timeout=10)
 (ROOT/'plugin-validation.log').write_bytes(validation.stdout+validation.stderr)
 assert validation.returncode==0,'native plugin validation failed'
 end=time.monotonic()+10
 while not socket.exists():
  assert daemon.poll() is None and time.monotonic()<end,'daemon readiness';time.sleep(.02)
 base=[str(NATIVE),'-p','--output-format','stream-json','--verbose','--tools','ToolSearch,Agent','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--setting-sources','','--plugin-dir',str(ROOT/'plugin')]
 session='11111111-1111-4111-8111-111111111111'
 fork_session='22222222-2222-4222-8222-222222222222'
 child_session='33333333-3333-4333-8333-333333333333'
 for stage,flags in [('start',['--session-id',session]),('resume',['--resume',session]),
     ('fork',['--resume',session,'--fork-session','--session-id',fork_session]),
     ('parent_later',['--resume',session]),('child',['--session-id',child_session])]:
  count=0;child=subprocess.Popen(base+flags,cwd=workspace,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
  try:stdout,stderr=child.communicate(('PRIVATE_CALLER_INPUT '+stage).encode(),timeout=30)
  finally:
   if child.poll() is None:
    os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=5)
  (ROOT/(stage+'.stdout')).write_bytes(stdout);(ROOT/(stage+'.stderr')).write_bytes(stderr)
  assert child.returncode==0 and (4<=count<=8 if stage=='child' else count==2),(stage,child.returncode,count)
  if stage=='child': assert child_result_seen,'native child fixture tool did not execute'
  else: assert b'native caller probe' in stdout,{'stage':stage,'native_output':stdout[-4000:].decode(errors='replace'),'native_error':stderr[-1000:].decode(errors='replace')}
  checks.append({'stage':stage,'pid':child.pid,'requests':count,'stdout_sha256':hashlib.sha256(stdout).hexdigest()})
 assert not errors,errors
 records=[json.loads(l) for l in (ROOT/'bridge.jsonl').read_text().splitlines()] if (ROOT/'bridge.jsonl').exists() else []
 bindings=[r for r in records if r['event']['op']=='bind'];assert len(bindings)==5,'native hook did not create all immutable calls'
 for r,expected_session in zip(bindings[:4],[session,session,fork_session,session],strict=True):
  assert 'error' not in r['response'],r['response']
  b=r['response']['data'];assert b['caller']['session']==expected_session and b['caller']['agent']=='main' and b['turn_id'] and b['tool_call_id']
 assert bindings[0]['response']['data']['turn_id']!=bindings[1]['response']['data']['turn_id']
 native_child=bindings[4]['response']['data']
 assert native_child['caller']['session']==child_session and native_child['caller']['agent']!='main'
 assert native_child['proof']['kind']=='registered_child' and native_child['proof']['parent']==child_session
 syncs=[r['response']['data']['id'] for r in records if r['event']['op']=='sync'];assert syncs,'native turn did not submit backfill'
 for job in syncs:
  end=time.monotonic()+15
  while True:
   result=subprocess.run(argv+['job','show',job],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5);assert result.returncode==0
   data=json.loads(result.stdout)['data']
   if data['state'] not in ['running','queued']:break
   assert time.monotonic()<end;time.sleep(.02)
  assert data['state']=='succeeded' and data['result']['checkpoint']['checkpoint']['pending'] is None,data
 installed=ROOT/'installed plugin';installed_session='44444444-4444-4444-8444-444444444444'
 install=subprocess.run(argv+['adapter','install','claude','--binary',str(NATIVE),'--directory',str(installed)],env=env,capture_output=True,timeout=10)
 assert install.returncode==0,install.stderr
 launch=json.loads(install.stdout)['data']['launch_args']
 validation=subprocess.run([str(NATIVE),'plugin','validate',str(installed)],env=env,capture_output=True,timeout=10)
 assert validation.returncode==0,validation.stdout+validation.stderr
 stage='installed';count=0
 child=subprocess.Popen(launch+['-p','--output-format','stream-json','--verbose','--tools','','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--setting-sources','','--session-id',installed_session],cwd=workspace,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
 try:stdout,stderr=child.communicate(b'PRIVATE_INSTALLED_PLUGIN_INPUT',timeout=30)
 finally:
  if child.poll() is None:
   os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=5)
 assert child.returncode==0 and count==1 and b'Observed caller installed' in stdout,(child.returncode,count,stderr)
 stage='tui';count=0;tui_session='55555555-5555-4555-8555-555555555555'
 tui_check=tui(launch+['PRIVATE_TUI_INPUT','--session-id',tui_session,'--tools','','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--setting-sources',''],'Observed caller tui')
 tui_requests=[r for r in requests if r['stage']=='tui']
 assert sum(r['kind']=='conversation' for r in tui_requests)==1 and sum(r['kind']=='title' for r in tui_requests)<=1
 end=time.monotonic()+15
 while True:
  result=subprocess.run(argv+['job','list'],env=env,capture_output=True,timeout=5);assert result.returncode==0
  page=json.loads(result.stdout)['data'];assert page['next'] is None,'fixture exceeds one job page'
  jobs=[j for j in page['items'] if j.get('binding',{}).get('source',{}).get('session') in [installed_session,tui_session]]
  if {j['binding']['source']['session'] for j in jobs}=={installed_session,tui_session} and all(j['state'] not in ['queued','running'] for j in jobs):break
  if time.monotonic()>=end:
   diagnostic=[]
   for job in jobs:
    raw=pathlib.Path(job['path']).read_bytes();rows=[json.loads(row) for row in raw.splitlines()]
    checkpoint=(job.get('result') or {}).get('checkpoint',{}).get('checkpoint',{})
    offset=json.loads(checkpoint.get('cursor') or '{}').get('offset',0)
    diagnostic.append({'state':job['state'],'session':job['binding']['source']['session'],'target':job['binding']['until_event'],'target_in_file':any(r.get('uuid')==job['binding']['until_event'] for r in rows),'pending':checkpoint.get('pending'),'offset':offset,'bytes':len(raw),'next_frame':raw[offset:].splitlines()[0][:2048].decode(errors='replace') if offset<len(raw) else None,'native_types':sorted({r.get('type','') for r in rows})})
   raise AssertionError(diagnostic)
  time.sleep(.02)
 failures=[]
 for job in jobs:
  checkpoint=job['result']['checkpoint']['checkpoint']
  if job['state']!='succeeded' or checkpoint['pending'] is not None:
   offset=json.loads(checkpoint['cursor'])['offset']
   native_rows=pathlib.Path(job['path']).read_bytes().splitlines()
   failures.append({'state':job['state'],'error':job['error'],'next_frame':pathlib.Path(job['path']).read_bytes()[offset:].splitlines()[0][:2048].decode(errors='replace'),'native_types':sorted({json.loads(row).get('type','') for row in native_rows})})
 assert not failures,failures
 traces=[]
 for identity,prompt,answer in [(installed_session,'PRIVATE_INSTALLED_PLUGIN_INPUT','Observed caller installed'),(tui_session,'PRIVATE_TUI_INPUT','Observed caller tui')]:
  job=max((j for j in jobs if j['binding']['source']['session']==identity),key=lambda j:json.loads(j['result']['checkpoint']['checkpoint']['cursor'])['offset'])
  cursor=json.loads(job['result']['checkpoint']['checkpoint']['cursor'])
  captured=pathlib.Path(job['path']).read_bytes()[:cursor['offset']]
  assert hashlib.sha256(captured[:4096]).hexdigest()==cursor['prefix_sha256']
  assert hashlib.sha256(captured[-4096:]).hexdigest()==cursor['tail_sha256']
  rows=[json.loads(row) for row in captured.splitlines()]
  inputs=[r['uuid'] for r in rows if r['type']=='user' and prompt in json.dumps(r.get('message'))]
  outputs=[r['uuid'] for r in rows if r['type']=='assistant' and answer in json.dumps(r.get('message'))]
  assert len(inputs)==len(outputs)==1,(identity,inputs,outputs)
  traces.append({'session':identity,'input_uuid':inputs[0],'output_uuid':outputs[0],'captured_bytes':len(captured),'captured_sha256':hashlib.sha256(captured).hexdigest(),'checkpoint_revision':job['result']['checkpoint']['revision']})
 doctor=subprocess.run(argv+['doctor','--host','claude','--binary',str(NATIVE),'--directory',str(installed)],env=env,capture_output=True,timeout=10)
 assert doctor.returncode==0 and json.loads(doctor.stdout)['installation']['state']=='verified',{'doctor_error':doctor.stderr.decode(),'plugin_files':sorted(str(p.relative_to(installed)) for p in installed.rglob('*'))}
 removal=subprocess.run(argv+['adapter','uninstall','claude','--directory',str(installed)],env=env,capture_output=True,timeout=10)
 assert removal.returncode==0,removal.stderr
 assert json.loads(removal.stdout)['data']['preserved_paths']==['.claude-plugin/types','tsconfig.json']
 assert (installed/'.claude-plugin/types/claude-code/index.d.ts').is_file() and (installed/'tsconfig.json').is_file()
 assert not (installed/'hooks').exists() and not (installed/'.claude-plugin/plugin.json').exists() and not (installed/'akasha-install.json').exists()
 checks.append({'stage':'installed','requests':1,'capture_jobs':sum(j['binding']['source']['session']==installed_session for j in jobs),'stdout_sha256':hashlib.sha256(stdout).hexdigest(),'uninstalled':True})
 tui_check['capture_jobs']=sum(j['binding']['source']['session']==tui_session for j in jobs);checks.append(tui_check)
 assert not errors,errors
 print(json.dumps({'native_sha256':hashlib.sha256(NATIVE.read_bytes()).hexdigest(),'akasha_sha256':hashlib.sha256(AKASHA.read_bytes()).hexdigest(),'native_bindings':len(bindings),'native_backfill_jobs':len(syncs),'checks':checks,'traces':traces,'authenticated_model_acceptance':False}),flush=True)
finally:
 server.shutdown();thread.join(timeout=2);server.server_close()
 try:
  if daemon is not None and daemon.poll() is None:
   try:
    subprocess.run(argv+['daemon','stop'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
    daemon.wait(timeout=15)
   except subprocess.TimeoutExpired:
    os.killpg(daemon.pid,signal.SIGKILL);daemon.wait(timeout=5)
 finally:
  daemon_log.close()
  shutil.rmtree(socket_root)
  shutil.rmtree(ROOT)
