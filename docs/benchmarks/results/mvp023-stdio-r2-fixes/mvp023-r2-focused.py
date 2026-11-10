import subprocess, sys
commands = [
 ["python3", "scripts/test_notifier_file.py"],
 ["cargo", "test", "--locked", "--lib", "notifier", "--", "--nocapture"],
 ["cargo", "test", "--locked", "--test", "config", "notifier", "--", "--nocapture"],
 ["cargo", "test", "--locked", "--test", "daemon", "notifier_tests::", "--", "--nocapture"],
 ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
]
for command in commands:
 print("COMMAND", command, flush=True)
 result = subprocess.run(command)
 print("EXIT", result.returncode, flush=True)
 if result.returncode: sys.exit(result.returncode)
