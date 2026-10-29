"""Delayed greeting diagnosis against the frozen product; no model or build."""
import concurrent.futures, hashlib, json, os, pathlib, secrets, socket, struct, subprocess, sys, threading, time

E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp016')
A = W / 'target/debug/akasha-otsuge'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
binary_sha = sha(A)

def case(budget_ms, delay_seconds):
    name = 'akasha-' + secrets.token_hex(32)
    token = 'b' * 64
    failures = []
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind('\0' + name)
        listener.listen(1)
        listener.settimeout(5)
        def serve():
            try:
                with listener.accept()[0] as stream:
                    stream.settimeout(5)
                    time.sleep(delay_seconds)
                    greeting = json.dumps({'protocol_version': 1, 'max_message_bytes': 2097152}).encode()
                    stream.sendall(struct.pack('!I', len(greeting)) + greeting)
                    def exact(n):
                        data = b''
                        while len(data) < n:
                            part = stream.recv(n-len(data))
                            if not part: raise EOFError('client closed')
                            data += part
                        return data
                    size = struct.unpack('!I', exact(4))[0]
                    assert 0 < size <= 16384
                    request = json.loads(exact(size))
                    assert request['command']['token'] == token
                    reply = json.dumps({'schema_version': 1, 'sequence': 1, 'request_id': request['request_id'], 'data': {'answer': 'queued response'}}).encode()
                    stream.sendall(struct.pack('!I', len(reply)) + reply)
            except (BrokenPipeError, ConnectionResetError, EOFError, TimeoutError) as exc:
                failures.append(type(exc).__name__)
        worker = threading.Thread(target=serve)
        worker.start()
        start = time.monotonic()
        result = subprocess.run([str(A), 'adapter', 'pi-context'],
            input=json.dumps({'op':'context','request':{'operation':'describe'}}),
            text=True, capture_output=True, timeout=budget_ms/1000+5,
            env=dict(HOME=os.environ['HOME'], PATH='/usr/bin:/bin', AKASHA_PI_RUN_SOCKET=name,
                AKASHA_PI_RUN_TOKEN=token, AKASHA_RUN_DEADLINE_MS=str(time.time_ns()//1000000+budget_ms)))
        elapsed = time.monotonic()-start
        worker.join(timeout=5)
        assert not worker.is_alive()
    return {'budget_ms':budget_ms,'greeting_delay_seconds':delay_seconds,
        'exit_code':result.returncode,'client_elapsed_seconds':elapsed,
        'stdout':result.stdout,'stderr':result.stderr,'server_errors':failures}

with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    results = list(pool.map(lambda v:case(*v), [(30000,11),(2000,2.5)]))
assert sha(A) == binary_sha
receipt = {'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip(),
    'binary_sha256':binary_sha,'harness_sha256':sha(pathlib.Path(__file__)),
    'binaries_unchanged':True,'cases':results}
with (E / (sys.argv[1]+'.json')).open('x') as out:
    json.dump(receipt,out,indent=2)
print(json.dumps(receipt),flush=True)
