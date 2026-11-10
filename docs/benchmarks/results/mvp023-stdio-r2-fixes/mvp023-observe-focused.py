import collections, gzip, json, os, pathlib, subprocess, sys, time
E = pathlib.Path(__file__).parent
name, selector = sys.argv[1:]
command = ["cargo", "test", "--locked", "--test", "daemon", selector, "--", "--nocapture"]
env = dict(os.environ, RUST_BACKTRACE="1")
start = time.monotonic()
print(json.dumps({"command":command,"diagnostic":"unchanged source, inherited test concurrency, content-free owned-process and PSI snapshots"}), flush=True)
child = subprocess.Popen(command, env=env)
def read(path):
 try: return path.read_text().strip()
 except (FileNotFoundError, ProcessLookupError, PermissionError): return None
with gzip.open(E / (name + "-samples.jsonl.gz"), "xt") as output:
 while child.poll() is None:
  pending = [child.pid]; seen = set(); rows = []
  while pending:
   pid = pending.pop()
   if pid in seen: continue
   seen.add(pid); root = pathlib.Path("/proc") / str(pid)
   try: tasks = list((root / "task").iterdir())
   except (FileNotFoundError, ProcessLookupError, PermissionError): continue
   states = collections.Counter(); waits = collections.Counter(); interesting = []
   for task in tasks:
    children = read(task / "children")
    if children: pending.extend(map(int, children.split()))
    stat = read(task / "stat")
    if not stat: continue
    fields = stat.rsplit(") ", 1)[1].split(); state = fields[0]; states[state] += 1
    wait = read(task / "wchan"); waits[wait] += 1
    syscall = read(task / "syscall")
    if state == "D" or syscall and syscall.split()[0] in ["74", "75"]:
     # Keep syscall number, not pointer values or file content.
     interesting.append({"tid":int(task.name),"state":state,"wchan":wait,"syscall":syscall.split()[0] if syscall else None,"schedstat":read(task / "schedstat")})
   rows.append({"pid":pid,"comm":read(root / "comm"),"states":dict(states),"waits":dict(waits),"io":read(root / "io"),"interesting":interesting})
  output.write(json.dumps({"elapsed":time.monotonic()-start,"processes":rows,"psi":{n:read(pathlib.Path("/proc/pressure") / n) for n in ["cpu","memory","io"]}}) + "\n")
  output.flush()
  time.sleep(0.5)
rc = child.wait()
print(json.dumps({"exit":rc,"seconds":time.monotonic()-start,"samples":name+"-samples.jsonl.gz"}), flush=True)
sys.exit(rc)
