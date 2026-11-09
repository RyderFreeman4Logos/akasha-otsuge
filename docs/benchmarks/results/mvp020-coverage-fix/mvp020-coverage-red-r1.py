import subprocess,json
cases=[['cargo','test','--test','daemon','editor_tests::editor_keeps_','--','--nocapture'],['cargo','test','--test','daemon','codex_tests::consult_conclusions_validate_evidence_no_hit_conditions_and_conflicts','--','--exact','--nocapture'],['cargo','test','--lib','research_retains_errors_after_a_successful_dispatch_without_swallowing_them','--','--nocapture']]
results=[]
for cmd in cases:
 print('CASE '+json.dumps(cmd),flush=True)
 results.append(dict(command=cmd,exit_code=subprocess.run(cmd).returncode))
print('RED_RESULTS '+json.dumps(results),flush=True)
assert all(r['exit_code']==101 for r in results),results
