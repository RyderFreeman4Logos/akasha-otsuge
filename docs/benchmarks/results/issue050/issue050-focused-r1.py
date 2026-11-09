import subprocess,json
commands=[['cargo','clean','--package','akasha-otsuge'],['cargo','test','--test','daemon','--','--nocapture'],['cargo','clippy','--all-targets','--','-D','warnings']]
for command in commands:
 print('CASE '+json.dumps(command),flush=True)
 subprocess.run(command,check=True)
