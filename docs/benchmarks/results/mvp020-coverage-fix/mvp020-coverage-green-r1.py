import subprocess,json
cases=[['cargo','test','--test','daemon','editor_tests::','--','--nocapture'],['cargo','test','--lib','daemon::jobs::consult::editor_tests::','--','--nocapture'],['cargo','test','--test','daemon','consult','--','--nocapture'],['cargo','clippy','--all-targets','--','-D','warnings']]
for cmd in cases:
 print('CASE '+json.dumps(cmd),flush=True)
 subprocess.run(cmd,check=True)
