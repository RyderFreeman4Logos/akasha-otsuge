"""Forward actual CPI requests; retain only tool names, code paths and error codes."""
import json, threading, urllib.request, urllib.error
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
class TraceProxy:
 def __init__(self):
  self.rows=[]; owner=self
  class Handler(BaseHTTPRequestHandler):
   def log_message(self,*args):pass
   def do_POST(self):
    assert self.path=='/v1/chat/completions'
    n=int(self.headers['Content-Length']);assert 0<n<=4194304
    body=self.rfile.read(n);data=json.loads(body)
    calls=[]
    for message in data.get('messages',[]):
     for call in message.get('tool_calls',[]):
      f=call.get('function',{});entry={'name':f.get('name')}
      if f.get('name') in ('code_read','code_search','delegate'):
       try:entry['args']=json.loads(f['arguments'])
       except ValueError:entry['arguments_valid']=False
      if entry not in calls:calls.append(entry)
    owner.rows.append({'model':data.get('model'),'tools':calls})
    headers={'Content-Type':'application/json','Authorization':self.headers.get('Authorization','')}
    request=urllib.request.Request('http://127.0.0.1:48317'+self.path,body,headers)
    try:reply=urllib.request.urlopen(request,timeout=300)
    except urllib.error.HTTPError as error:reply=error
    with reply:
     result=reply.read(8388609);assert len(result)<=8388608
     content=[];finish=[];usage=[]
     for line in result.decode('utf8',errors='replace').splitlines():
      if not line.startswith('data:'):continue
      try:event=json.loads(line[5:])
      except ValueError:continue
      if event.get('usage'):usage.append(event['usage'])
      for choice in event.get('choices',[]):
       text=choice.get('delta',{}).get('content')
       if isinstance(text,str):content.append(text)
       if choice.get('finish_reason'):finish.append(choice['finish_reason'])
     text=''.join(content)
     owner.rows.append({'response_status':reply.status,'finish':finish,'assistant_final_text':text[:16384],'assistant_text_truncated':len(text)>16384,'usage':usage})
     self.send_response(reply.status)
     self.send_header('Content-Type',reply.headers.get('Content-Type','application/json'))
     self.send_header('Content-Length',str(len(result)));self.end_headers();self.wfile.write(result)
  self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
  self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
  self.url='http://127.0.0.1:'+str(self.server.server_port)+'/v1'
 def close(self):
  self.server.shutdown();self.server.server_close();self.thread.join(5);assert not self.thread.is_alive()
