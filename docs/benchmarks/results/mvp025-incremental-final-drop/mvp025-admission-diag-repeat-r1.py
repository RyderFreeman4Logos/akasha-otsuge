import subprocess,json,time
for attempt in range(1,11):
    print(json.dumps({'diagnostic_attempt':attempt,'limit':10}),flush=True)
    r=subprocess.run(['cargo','test','--locked','--offline','--lib','storage::','--','--nocapture'])
    print(json.dumps({'diagnostic_attempt':attempt,'exit_code':r.returncode}),flush=True)
    if r.returncode:raise SystemExit(r.returncode)
