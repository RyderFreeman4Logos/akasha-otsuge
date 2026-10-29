use super::*;
use akasha_otsuge::adapter::claude::ClaudeRunner;
use std::time::Duration;

fn executable(f: &Fixture, body: &str) -> PathBuf {
    let path = f.0.join("fake-claude");
    super::executable::write(
        &path,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.311 (Claude Code)\\n'; exit 0; fi\n{body}"
        ),
    );
    path
}
async fn origin(f: &Fixture, store: &Storage) -> Invocation {
    Domain::new(store)
        .set_read_grant("claude-grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let tip = event(
        store,
        "root",
        "input",
        "PRIVATE_ORIGINAL_STAYS_OUT_OF_PROMPT",
        None,
        false,
    )
    .await;
    Invocation::bind(store, runtime(f, store, "root", tip, "call-claude"))
        .await
        .unwrap()
}
fn spec(invocation: &Invocation, timeout: Duration) -> RunSpec {
    RunSpec::new(
        invocation,
        RunOptions {
            job_id: "claude-job".into(),
            executor_profile: "claude".into(),
            instructions_version: "transport-test".into(),
            runner_timeout: timeout,
            args: ConsultArgs::new(Some("/help literal $(touch forbidden) 字".into())).unwrap(),
        },
        ReadCapability::issue(invocation).unwrap(),
        lease(invocation),
    )
    .unwrap()
}

#[tokio::test]
async fn native_runner_requires_init_terminal_and_exit_with_isolated_tool_flags() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = executable(
        &f,
        r#"printf '%s\n' "$@" > "$0.argv"
/bin/cat > "$0.stdin"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"runner-session","claude_code_version":"2.1.311","tools":["ToolSearch","mcp__akasha-runner__context_describe","mcp__akasha-runner__context_list","mcp__akasha-runner__context_read","mcp__akasha-runner__context_search","mcp__akasha-runner__memory_search","mcp__akasha-runner__memory_get","mcp__akasha-runner__code_search","mcp__akasha-runner__code_read","mcp__akasha-runner__delegate"],"mcp_servers":[{"name":"akasha-runner","status":"connected","source":"plugin"}]}'
printf '%s\n' '{"type":"assistant","session_id":"runner-session","message":{"id":"msg-1","usage":{"input_tokens":10,"output_tokens":1},"content":[{"type":"text","text":"PROVISIONAL_NOT_FINAL"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"runner-session","result":"final referenced answer","usage":{"input_tokens":10,"output_tokens":2}}'
"#,
    );
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "Transport test instructions",
    )
    .await
    .unwrap();
    let report = runner.probe().unwrap();
    assert_eq!(report.host, Host::ClaudeCode);
    assert!(report.cancellation && report.outer_timeout);
    let plan = spec(&invocation, Duration::from_secs(5));
    let shared = plan.lease_handle();
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&shared, 1000);
    while let Some(event) = handle.next().await.unwrap() {
        collector.push(event).unwrap();
    }
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "final referenced answer"
    );
    drop(handle);
    let args = std::fs::read_to_string(binary.with_extension("argv")).unwrap();
    let args: Vec<_> = args.lines().collect();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--tools", "ToolSearch"])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--setting-sources", ""])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--mcp-config", "{\"mcpServers\":{}}"])
    );
    assert!(args.contains(&"--strict-mcp-config"));
    assert!(args.contains(&"--plugin-dir"));
    assert!(!args.contains(&"--bare"));
    let prompt = std::fs::read_to_string(binary.with_extension("stdin")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&prompt).unwrap(),
        serde_json::json!({"hint":"/help literal $(touch forbidden) 字"})
    );
    assert!(!prompt.contains("PRIVATE_ORIGINAL"));
    assert!(!f.0.join("forbidden").exists());
    store.close().await.unwrap();
}

const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"runner-session","claude_code_version":"2.1.311","tools":["ToolSearch","mcp__akasha-runner__context_describe","mcp__akasha-runner__context_list","mcp__akasha-runner__context_read","mcp__akasha-runner__context_search","mcp__akasha-runner__memory_search","mcp__akasha-runner__memory_get","mcp__akasha-runner__code_search","mcp__akasha-runner__code_read","mcp__akasha-runner__delegate"],"mcp_servers":[{"name":"akasha-runner","status":"connected","source":"plugin"}]}"#;
const DONE: &str = r#"{"type":"result","subtype":"success","is_error":false,"session_id":"runner-session","result":"final answer","usage":{"input_tokens":10,"output_tokens":2}}"#;

fn no_live_process(pid: u32) -> bool {
    let path = format!("/proc/{pid}/stat");
    match std::fs::read_to_string(path) {
        Ok(stat) => matches!(
            stat.rsplit_once(')').unwrap().1.split_whitespace().next(),
            Some("Z" | "X")
        ),
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) =>
        {
            true
        }
        Err(e) => panic!("cannot inspect owned PID: {e}"),
    }
}
fn no_run_directory(f: &Fixture) -> bool {
    !std::fs::read_dir(&f.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("akasha-run-")
    })
}
fn sleeper(f: &Fixture) -> PathBuf {
    executable(
        f,
        &format!(
            r#"/bin/cat >/dev/null
printf '%s\n' "$$" > "$0.pid"
/bin/sleep 30 &
printf '%s\n' "$!" > "$0.child"
printf '%s\n' '{INIT}'
wait
"#
        ),
    )
}

#[tokio::test]
async fn native_runner_cancellation_is_terminal_and_reaps_owned_process_group() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = sleeper(&f);
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "instructions",
    )
    .await
    .unwrap();
    let plan = spec(&invocation, Duration::from_secs(5));
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Started)
    ));
    let pids: Vec<u32> = ["pid", "child"]
        .iter()
        .map(|ext| {
            std::fs::read_to_string(binary.with_extension(ext))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        })
        .collect();
    assert!(pids.iter().all(|pid| !no_live_process(*pid)));
    assert!(
        handle
            .cancel(CancelReason::User)
            .await
            .unwrap()
            .acknowledged
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while pids.iter().any(|pid| !no_live_process(*pid)) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(no_run_directory(&f));
    assert_eq!(handle.next().await.unwrap_err().code, ErrorCode::Cancelled);
    assert_ne!(handle.exit_status().await.unwrap(), Some(0));
    drop(handle);
    store.close().await.unwrap();
}

#[tokio::test]
async fn native_runner_refuses_incomplete_mismatched_unsafe_and_failed_streams() {
    let cases = [
        (
            format!(
                "printf '%s\\n' '{INIT}' '{{\"type\":\"result\",\"session_id\":\"runner-session\",\"subtype\":\"error_during_execution\",\"is_error\":true}}'"
            ),
            ErrorCode::RunnerFailed,
        ),
        (
            format!(
                "printf '%s\\n' '{INIT}' '{}'",
                DONE.replace("\"input_tokens\":10", "\"input_tokens\":200000")
            ),
            ErrorCode::BudgetExhausted,
        ),
        (
            format!("printf '%s\\n' '{DONE}'"),
            ErrorCode::ProtocolInvalid,
        ),
        (
            format!("printf '%s\\n' '{INIT}'"),
            ErrorCode::RunnerIncomplete,
        ),
        (
            format!("printf '%s\\n' '{INIT}' '{DONE}'; exit 7"),
            ErrorCode::RunnerFailed,
        ),
        (
            format!("printf '%s\\n' '{INIT}' '{INIT}'"),
            ErrorCode::ProtocolInvalid,
        ),
        (
            format!(
                "printf '%s\\n' '{INIT}' '{}'",
                DONE.replace("runner-session", "different-session")
            ),
            ErrorCode::ProtocolInvalid,
        ),
        (
            format!("printf '%s\\n' '{INIT}' '{DONE}' '{DONE}'"),
            ErrorCode::ProtocolInvalid,
        ),
        (
            format!("printf '%s\\n' '{INIT}'; printf '%s' '{DONE}'"),
            ErrorCode::ProtocolInvalid,
        ),
        (
            format!(
                "printf '%s\\n' '{}'",
                INIT.replace("\"ToolSearch\"", "\"Bash\"")
            ),
            ErrorCode::AdapterUnsupported,
        ),
        (
            format!(
                "printf '%s\\n' '{INIT}' '{{\"type\":\"assistant\",\"session_id\":\"runner-session\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"name\":\"Bash\"}}]}}}}'"
            ),
            ErrorCode::ToolDenied,
        ),
        (
            format!("printf '%s\\n' '{INIT}'; /bin/head -c 140000 /dev/zero"),
            ErrorCode::OutputLimit,
        ),
        (
            format!("printf '%s\\n' '{INIT}'; /bin/head -c 9000 /dev/zero >&2"),
            ErrorCode::OutputLimit,
        ),
    ];
    for (index, (body, expected)) in cases.iter().enumerate() {
        let f = Fixture::new();
        let store = f.open().await;
        let invocation = origin(&f, &store).await;
        let binary = executable(&f, &format!("/bin/cat >/dev/null\n{body}\n"));
        let mut runner = ClaudeRunner::new(
            &binary,
            std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
            &f.0,
            "instructions",
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "probe case {index}: {error:?}; file={:?}; version={:?}",
                std::fs::metadata(&binary),
                std::process::Command::new(&binary)
                    .arg("--version")
                    .output()
            )
        });
        let plan = spec(&invocation, Duration::from_secs(5));
        let mut context = CapturedContext::new(&store, invocation).unwrap();
        let mut handle = runner.start(plan, &mut context).await.unwrap();
        let error = loop {
            match handle.next().await {
                Err(error) => break error,
                Ok(Some(RunnerEvent::Started | RunnerEvent::Usage { .. })) => {}
                result => panic!("case {index} accepted invalid stream: {result:?}"),
            }
        };
        assert_eq!(&error.code, expected, "case {index}");
        assert!(no_run_directory(&f));
        drop(handle);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_runner_preserves_original_deadline_and_drop_cleans_up() {
    for abandon in [false, true] {
        let f = Fixture::new();
        let store = f.open().await;
        let invocation = origin(&f, &store).await;
        let binary = sleeper(&f);
        let mut runner = ClaudeRunner::new(
            &binary,
            std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
            &f.0,
            "instructions",
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "probe abandon={abandon}: {error:?}; file={:?}; version={:?}",
                std::fs::metadata(&binary),
                std::process::Command::new(&binary)
                    .arg("--version")
                    .output()
            )
        });
        let plan = spec(
            &invocation,
            if abandon {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(300)
            },
        );
        let clock = std::time::Instant::now();
        let mut context = CapturedContext::new(&store, invocation).unwrap();
        let mut handle = runner.start(plan, &mut context).await.unwrap();
        assert!(matches!(
            handle.next().await.unwrap(),
            Some(RunnerEvent::Started)
        ));
        let pids: Vec<u32> = ["pid", "child"]
            .iter()
            .map(|ext| {
                std::fs::read_to_string(binary.with_extension(ext))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap()
            })
            .collect();
        if !abandon {
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert_eq!(
                handle.next().await.unwrap_err().code,
                ErrorCode::RunnerTimeout
            );
            assert!(clock.elapsed() < Duration::from_secs(2));
        }
        drop(handle);
        tokio::time::timeout(Duration::from_secs(3), async {
            while pids.iter().any(|pid| !no_live_process(*pid)) || !no_run_directory(&f) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_runner_context_bridge_uses_bound_capability_and_rejects_forgery() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = executable(
        &f,
        r#"/bin/cat >/dev/null
exec /usr/bin/python3 - 2>"$0.stderr" <<'PY'
import json,os,socket,struct
names=['ToolSearch']+['mcp__akasha-runner__'+x for x in ['context_describe','context_list','context_read','context_search','memory_search','memory_get','code_search','code_read','delegate']]
print(json.dumps(dict(type='system',subtype='init',session_id='runner-session',claude_code_version='2.1.311',tools=names,mcp_servers=[dict(name="akasha-runner",status="connected",source="plugin")])),flush=True)
def call(request,token=None):
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(os.environ['AKASHA_RUN_SOCKET'])
        data=json.dumps(dict(token=token if token is not None else os.environ['AKASHA_RUN_TOKEN'],request=request)).encode()
        s.sendall(struct.pack('>I',len(data))+data)
        def take(n):
            result=b''
            while len(result)<n:
                part=s.recv(n-len(result))
                assert part
                result+=part
            return result
        reply=json.loads(take(struct.unpack('>I',take(4))[0]))
        print(request,reply,file=__import__('sys').stderr,flush=True)
        return reply
assert call(dict(operation='describe'),'wrong')['error']['code']=='READ_DENIED'
assert call(dict(operation='read',handle='invented',start=0,max_bytes=500))['error']['code']=='READ_DENIED'
assert call(dict(operation='describe',session='forged'))['error']['code']=='PROTOCOL_INVALID'
page=call(dict(operation='list',cursor=None,limit=16))['data']['value']
assert call(dict(operation='read',handle=page['events'][0]['handle'],start=0,max_bytes=16385))['error']['code']=='INVALID_ARGUMENT'
result=call(dict(operation='read',handle=page['events'][0]['handle'],start=0,max_bytes=16384))['data']['value']['text']
assert result=='PRIVATE_ORIGINAL_STAYS_OUT_OF_PROMPT'
print(json.dumps(dict(type='result',subtype='success',is_error=False,session_id='runner-session',result='read-authorized-original',usage=dict(input_tokens=10,output_tokens=2))),flush=True)
PY
"#,
    );
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "instructions",
    )
    .await
    .unwrap();
    let plan = spec(&invocation, Duration::from_secs(5));
    let shared = plan.lease_handle();
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&shared, 1000);
    while let Some(event) = handle.next().await.unwrap_or_else(|error| {
        panic!(
            "{error:?}; fixture={}",
            std::fs::read_to_string(binary.with_extension("stderr")).unwrap_or_default()
        )
    }) {
        collector.push(event).unwrap();
    }
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "read-authorized-original"
    );
    drop(handle);
    assert!(no_run_directory(&f));
    store.close().await.unwrap();
}

#[tokio::test]
async fn native_runner_removes_owned_setup_when_executor_disappears_after_probe() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = f.0.join("fake-claude");
    super::executable::write(
        &binary,
        "#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n/bin/rm -- \"$0\"\n",
    );
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "instructions",
    )
    .await
    .unwrap();
    assert!(!binary.exists());
    let plan = spec(&invocation, Duration::from_secs(5));
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let error = match runner.start(plan, &mut context).await {
        Ok(_) => panic!("disappeared executable started"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::ExecutorUnavailable);
    assert!(
        no_run_directory(&f),
        "failed spawn leaked owned plugin directory"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn native_runner_normalizes_native_authentication_failure_without_echoing_output() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = executable(&f, "/bin/cat >/dev/null\n/bin/cat \"$0.frames\"\nexit 1\n");
    std::fs::write(
        binary.with_extension("frames"),
        format!(
            "{INIT}\n{}",
            include_str!("../fixtures/claude/2.1.311/authentication-failed.json")
        ),
    )
    .unwrap();
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "instructions",
    )
    .await
    .unwrap();
    let plan = spec(&invocation, Duration::from_secs(5));
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Started)
    ));
    let error = handle.next().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::AuthRequired);
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("Please run")
    );
    drop(handle);
    assert!(no_run_directory(&f));
    store.close().await.unwrap();
}

#[tokio::test]
async fn native_runner_pairs_recorded_native_tool_results() {
    let rows: Vec<serde_json::Value> =
        include_str!("../fixtures/claude/2.1.311/native-tool-roundtrip.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    for case in 0..6 {
        let mut frames = rows.clone();
        // Retain the recorded frames; only the controlled process advertises the current inventory.
        frames[0]["tools"] =
            serde_json::from_str::<serde_json::Value>(INIT).unwrap()["tools"].clone();
        let expected = match case {
            0 => None,
            1 => {
                frames[2]["message"]["content"][0]["tool_use_id"] = "unknown-tool".into();
                Some(ErrorCode::ProtocolInvalid)
            }
            2 => {
                frames.insert(3, frames[2].clone());
                Some(ErrorCode::ProtocolInvalid)
            }
            3 => {
                frames.remove(2);
                Some(ErrorCode::RunnerIncomplete)
            }
            4 => {
                frames[3]["message"]["content"][0]["id"] = "toolu_list".into();
                Some(ErrorCode::ProtocolInvalid)
            }
            _ => {
                frames[1]["parent_tool_use_id"] = "unregistered-child".into();
                Some(ErrorCode::ProtocolInvalid)
            }
        };
        let f = Fixture::new();
        let store = f.open().await;
        let invocation = origin(&f, &store).await;
        let binary = executable(&f, "/bin/cat >/dev/null\n/bin/cat \"$0.frames\"\n");
        std::fs::write(
            binary.with_extension("frames"),
            frames
                .iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let mut runner = ClaudeRunner::new(
            &binary,
            std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
            &f.0,
            "instructions",
        )
        .await
        .unwrap();
        let plan = spec(&invocation, Duration::from_secs(5));
        let mut context = CapturedContext::new(&store, invocation).unwrap();
        let mut handle = runner.start(plan, &mut context).await.unwrap();
        let outcome = loop {
            match handle.next().await {
                Err(error) => break Err(error.code),
                Ok(Some(RunnerEvent::Completed { output })) => break Ok(output),
                Ok(Some(_)) => {}
                Ok(None) => panic!("stream ended without outcome"),
            }
        };
        match expected {
            Some(code) => assert_eq!(outcome, Err(code), "case {case}"),
            None => assert_eq!(outcome, Ok("native authorized original verified".into())),
        }
        drop(handle);
        assert!(no_run_directory(&f));
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_runner_reports_native_retry_without_exposing_provider_details() {
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let binary = executable(&f, "/bin/cat >/dev/null\n/bin/cat \"$0.frames\"\n");
    std::fs::write(
        binary.with_extension("frames"),
        format!(
            "{INIT}\n{}{DONE}\n",
            include_str!("../fixtures/claude/2.1.311/api-retry.json")
        ),
    )
    .unwrap();
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "instructions",
    )
    .await
    .unwrap();
    let plan = spec(&invocation, Duration::from_secs(5));
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Started)
    ));
    assert_eq!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Progress {
            message: "Provider retry".into()
        })
    );
    assert_eq!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Usage {
            input_tokens: 10,
            output_tokens: 2
        })
    );
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Completed { .. })
    ));
    drop(handle);
    assert!(no_run_directory(&f));
    store.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires pinned native Claude and an isolated local Anthropic protocol fixture"]
async fn native_runner_local_protocol_round_trip() {
    let endpoint = url::Url::parse(&std::env::var("ANTHROPIC_BASE_URL").unwrap()).unwrap();
    assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
    assert_eq!(
        std::env::var("ANTHROPIC_API_KEY").unwrap(),
        "akasha-protocol-fixture-not-real"
    );
    let binary = PathBuf::from(std::env::var_os("AKASHA_TEST_CLAUDE_BINARY").unwrap());
    let f = Fixture::new();
    let store = f.open().await;
    let invocation = origin(&f, &store).await;
    let mut runner = ClaudeRunner::new(
        &binary,
        std::path::Path::new(env!("CARGO_BIN_EXE_akasha-otsuge")),
        &f.0,
        "Use context_list, then context_read for the original. Return the final referenced answer.",
    )
    .await
    .unwrap();
    let plan = spec(&invocation, Duration::from_secs(20));
    let shared = plan.lease_handle();
    let mut context = CapturedContext::new(&store, invocation).unwrap();
    let mut handle = runner.start(plan, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&shared, 1000);
    while let Some(event) = handle.next().await.unwrap() {
        collector.push(event).unwrap();
    }
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "native authorized original verified"
    );
    drop(handle);
    assert!(no_run_directory(&f));
    store.close().await.unwrap();
}
