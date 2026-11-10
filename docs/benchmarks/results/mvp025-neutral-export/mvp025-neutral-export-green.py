import subprocess,sys
commands=[['cargo','test','--locked','--test','archive','--test','storage','--test','cli'],['cargo','clippy','--locked','--workspace','--all-targets','--','-D','warnings'],['git','diff','--check']]
for command in commands:
 print('COMMAND',repr(command),flush=True)
 result=subprocess.run(command)
 if result.returncode: sys.exit(result.returncode)
