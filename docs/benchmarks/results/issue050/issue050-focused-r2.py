import subprocess,json
commands=[['cargo','test','--test','daemon','--','--nocapture'],['cargo','clippy','--all-targets','--','-D','warnings']]
for command in commands:
 print('CASE '+json.dumps(command),flush=True)
 subprocess.run(command,check=True)
