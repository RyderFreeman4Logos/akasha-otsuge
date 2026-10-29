import datetime, hashlib, http.server, json, os, pathlib, shlex, signal, subprocess, sys, threading, time, traceback

# Run after cargo test --test adapter_context --no-run. No real provider is used.
NATIVE, TEST, ROOT = [pathlib.Path(arg).resolve() for arg in sys.argv[1:]]
OUT = ROOT
NAME = 'protocol'
assert NATIVE.is_file() and TEST.is_file() and not ROOT.exists()
KEY = 'akasha-protocol-fixture-not-real'
ROOT.mkdir(mode=0o700)
for name in ('home', 'config', 'tmp'):
    (ROOT / name).mkdir(mode=0o700)
(ROOT / 'config/.claude.json').write_text(json.dumps({'hasCompletedOnboarding': True, 'lastOnboardingVersion': '2.1.311', 'customApiKeyResponses': {'approved': [KEY[-20:]], 'rejected': []}}))
wrapper = ROOT / 'claude'
wrapper.write_text('#!/bin/bash\nset -o pipefail\nif [ "$1" = --version ]; then exec ' + shlex.quote(str(NATIVE)) + ' "$@"; fi\n' + shlex.quote(str(NATIVE)) + ' "$@" | /usr/bin/tee ' + shlex.quote(str(ROOT / 'native.stdout')) + '\n')
wrapper.chmod(0o700)
requests, errors = [], []

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        try:
            size = int(self.headers['Content-Length'])
            assert 0 < size < 1024 * 1024
            request = json.loads(self.rfile.read(size))
            if self.path.startswith('/v1/messages/count_tokens'):
                payload = b'{"input_tokens":20}'
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
                return
            assert self.path.startswith('/v1/messages') and request['stream'] is True
            assert self.headers.get('x-api-key') == KEY
            requests.append(request)
            (ROOT / 'requests.json').write_text(json.dumps(requests, indent=2))
            number = len(requests)
            text = json.dumps(request['messages'], ensure_ascii=False)
            assert number <= 3
            if number == 1:
                assert '/help literal $(touch forbidden) 字' in text
                assert 'PRIVATE_ORIGINAL_STAYS_OUT_OF_PROMPT' not in text
                assert 'mcp__akasha-runner__context_list' in [t['name'] for t in request['tools']]
                read_tool = next(t for t in request['tools'] if t['name'] == 'mcp__akasha-runner__context_read')
                assert read_tool['input_schema']['properties']['max_bytes']['maximum'] == 16384, 'native read schema exceeds the shared reader limit'
                block = dict(type='tool_use', id='toolu_list', name='mcp__akasha-runner__context_list', input={})
                delta = dict(type='input_json_delta', partial_json='{"limit":16}')
            elif number == 2:
                results = [b for m in request['messages'] if isinstance(m['content'], list) for b in m['content'] if b['type'] == 'tool_result']
                result = next(b for b in results if b['tool_use_id'] == 'toolu_list')
                assert not result.get('is_error')
                content = result['content']
                data = json.loads(content if isinstance(content, str) else ''.join(b['text'] for b in content))
                handle = data['value']['events'][0]['handle']
                assert 'PRIVATE_ORIGINAL_STAYS_OUT_OF_PROMPT' not in text
                block = dict(type='tool_use', id='toolu_read', name='mcp__akasha-runner__context_read', input={})
                delta = dict(type='input_json_delta', partial_json=json.dumps(dict(handle=handle, start=0, max_bytes=16384)))
            else:
                results = [b for m in request['messages'] if isinstance(m['content'], list) for b in m['content'] if b['type'] == 'tool_result']
                result = next(b for b in results if b['tool_use_id'] == 'toolu_read')
                assert not result.get('is_error') and 'PRIVATE_ORIGINAL_STAYS_OUT_OF_PROMPT' in json.dumps(result)
                block = dict(type='text', text='')
                delta = dict(type='text_delta', text='native authorized original verified')
            message = dict(id='msg_fixture_' + str(number), type='message', role='assistant', model=request['model'], content=[], stop_reason=None, stop_sequence=None, usage=dict(input_tokens=20, output_tokens=1))
            events = [dict(type='message_start', message=message), dict(type='content_block_start', index=0, content_block=block), dict(type='content_block_delta', index=0, delta=delta), dict(type='content_block_stop', index=0), dict(type='message_delta', delta=dict(stop_reason='end_turn' if number == 3 else 'tool_use', stop_sequence=None), usage=dict(output_tokens=20)), dict(type='message_stop')]
            payload = ''.join('event: ' + e['type'] + '\ndata: ' + json.dumps(e) + '\n\n' for e in events).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except Exception:
            errors.append(traceback.format_exc())
            self.send_error(400)

server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
thread = threading.Thread(target=server.serve_forever)
thread.start()
env = dict(HOME=str(ROOT / 'home'), USER='obj', LOGNAME='obj', PATH='/usr/bin:/bin', LANG='C.UTF-8', TMPDIR=str(ROOT / 'tmp'), CLAUDE_CONFIG_DIR=str(ROOT / 'config'), DISABLE_AUTOUPDATER='1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1', CLAUDE_CODE_DISABLE_CLAUDE_MDS='1', ANTHROPIC_API_KEY=KEY, ANTHROPIC_BASE_URL='http://127.0.0.1:' + str(server.server_port), AKASHA_TEST_CLAUDE_BINARY=str(wrapper))
start = time.monotonic()
child = subprocess.Popen([str(TEST), 'claude_runner::native_runner_local_protocol_round_trip', '--ignored', '--exact', '--nocapture'], env=env, cwd=ROOT, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
try:
    output, _ = child.communicate(timeout=35)
except subprocess.TimeoutExpired:
    os.killpg(child.pid, signal.SIGKILL)
    output, _ = child.communicate()
    errors.append('test_outer_timeout')
finally:
    server.shutdown()
    thread.join(timeout=2)
    server.server_close()
(OUT / (NAME + '.test.log')).write_bytes(output)
summary = dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), native_sha256=hashlib.sha256(NATIVE.read_bytes()).hexdigest(), test_sha256=hashlib.sha256(TEST.read_bytes()).hexdigest(), test_pid=child.pid, exit_code=child.returncode, seconds=time.monotonic()-start, request_count=len(requests), errors=errors, scratch=str(ROOT), authenticated_model_acceptance=False)
(OUT / (NAME + '.summary.json')).write_text(json.dumps(summary, indent=2) + '\n')
print(output.decode(errors='replace'), flush=True)
print(json.dumps(summary), flush=True)
assert child.returncode == 0 and len(requests) == 3 and not errors
