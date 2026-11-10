import json,subprocess,sys,time
steps=[['cargo','fmt','--all'],
 ['cargo','test','--locked','--offline','--lib','storage::','--','--nocapture'],
 ['cargo','test','--locked','--offline','--test','archive','--','--nocapture'],
 ['cargo','test','--locked','--offline','--test','storage','--','--nocapture'],
 ['cargo','test','--locked','--offline','--lib','daemon::jobs::import_tests','--','--nocapture'],
 ['cargo','test','--locked','--offline','--test','daemon','import','--','--nocapture'],
 ['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'],
 ['git','diff','--check']]
results=[]
for command in steps:
 print('COMMAND '+json.dumps(command),flush=True);start=time.monotonic();r=subprocess.run(command)
 row={'command':command,'exit_code':r.returncode,'seconds':time.monotonic()-start};results.append(row);print('RESULT '+json.dumps(row),flush=True)
 if r.returncode:break
print('ALL_RESULTS '+json.dumps(results),flush=True)
sys.exit(results[-1]['exit_code'])
