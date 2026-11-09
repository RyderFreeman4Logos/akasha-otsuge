import subprocess
subprocess.run(['cargo','fmt','--all'],check=True)
raise SystemExit(subprocess.run(['cargo','test','--locked','--lib','daemon::jobs::consult::editor_tests::editor_legacy_checkpoint_without_coverage_stays_for_review','--','--exact']).returncode)
