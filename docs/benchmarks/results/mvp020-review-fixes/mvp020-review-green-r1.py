import hashlib, json, pathlib, subprocess
E=pathlib.Path(__file__).parent
commands=[['cargo','fmt','--all'],['cargo','test','--locked','--test','daemon','editor_tests::'],['cargo','test','--locked','--lib','daemon::jobs::consult::editor_tests::'],['cargo','test','--locked','--test','daemon','consult_'],['cargo','clippy','--locked','--workspace','--all-targets','--','-D','warnings']]
for command in commands:
 print(json.dumps({'command':command}),flush=True)
 result=subprocess.run(command)
 if result.returncode: raise SystemExit(result.returncode)
print('Focused editor and recovery regressions, Consult siblings and Clippy PASS',flush=True)
