import hashlib, json, pathlib, subprocess, sys
E=pathlib.Path(__file__).parent
subprocess.run(['cargo','fmt','--all'],check=True)
diff=subprocess.check_output(['git','diff','HEAD','--binary'])
(E/'mvp020-review-red-r1.patch').write_bytes(diff)
results=[]
for command in [
 ['cargo','test','--locked','--test','daemon','editor_tests::'],
 ['cargo','test','--locked','--lib','daemon::jobs::consult::editor_tests::editor_checkpoint_and_committed_patch_survive_sigkill_without_runner_replay','--','--exact'],
]:
 print(json.dumps({'command':command}),flush=True)
 rc=subprocess.run(command).returncode
 results.append({'command':command,'exit_code':rc})
receipt={'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'test_patch_sha256':hashlib.sha256(diff).hexdigest(),'results':results}
(E/'mvp020-review-red-r1-results.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt),flush=True)
raise SystemExit(0 if all(row['exit_code']==101 for row in results) else 1)
