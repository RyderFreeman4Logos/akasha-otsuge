use akasha_otsuge::adapter::*;
use akasha_otsuge::capture::CoverageLevel;
use akasha_otsuge::config::{
    Host,
    deadline::{Deadline, Layer},
};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

fn report() -> CapabilityReport {
    CapabilityReport {
        host: Host::Codex,
        version: Some("fixture-1".into()),
        executable: Some(PathBuf::from("/fixture/codex")),
        adapter_version: "contract-1".into(),
        binding: BindingLevel::SessionOnly,
        native_children: false,
        history: CoverageLevel::VisibleText,
        machine_protocol: Some("fixture-jsonl-1".into()),
        cancellation: true,
        outer_timeout: true,
        capabilities: BTreeSet::from(["context.read".into(), "runner.events".into()]),
        self_test: SelfTest::NotRun,
    }
}
fn lease() -> RunLease {
    RunBudget::new(
        "origin",
        "fence",
        Arc::new(Semaphore::new(1)),
        BudgetLimits::default(),
        Deadline::start(Layer::Workflow, Duration::from_secs(10), None).unwrap(),
    )
    .unwrap()
    .start()
    .unwrap()
}

#[test]
fn capability_negotiation_refuses_required_unknowns_and_reports_optional_degradation() {
    let r = report();
    assert_eq!(
        r.negotiate(&["context.read"], &["future.capability"])
            .unwrap()
            .unsupported_optional,
        vec!["future.capability"]
    );
    assert_eq!(
        r.negotiate(&["future.capability"], &[]).unwrap_err().code,
        ErrorCode::AdapterUnsupported
    );
    assert_eq!(
        r.require_binding(BindingLevel::ExactTurn, true)
            .unwrap_err()
            .code,
        ErrorCode::AdapterUnsupported
    );
    assert!(r.require_binding(BindingLevel::SessionOnly, false).is_ok());
    let mut invented = r.clone();
    invented.capabilities.insert("future.capability".into());
    assert!(
        invented.negotiate(&["future.capability"], &[]).is_err(),
        "both ends must implement a required capability"
    );
}

#[test]
fn only_terminal_result_and_successful_exit_produce_public_output() {
    for event in [
        RunnerEvent::Accepted,
        RunnerEvent::Started,
        RunnerEvent::Progress {
            message: "private trace".into(),
        },
    ] {
        let permit = lease();
        let mut c = RunCollector::new(&permit, 100);
        c.push(event).unwrap();
        assert_eq!(
            c.finish(Some(0)).unwrap_err().code,
            ErrorCode::RunnerIncomplete
        );
    }
    for exit in [None, Some(1), Some(137)] {
        let permit = lease();
        let mut c = RunCollector::new(&permit, 100);
        c.push(RunnerEvent::Started).unwrap();
        c.push(RunnerEvent::Completed {
            output: "answer".into(),
        })
        .unwrap();
        assert_eq!(c.finish(exit).unwrap_err().code, ErrorCode::RunnerFailed);
    }
    let permit = lease();
    let mut c = RunCollector::new(&permit, 100);
    c.push(RunnerEvent::Accepted).unwrap();
    c.push(RunnerEvent::Started).unwrap();
    c.push(RunnerEvent::Progress {
        message: "never inject this trace".into(),
    })
    .unwrap();
    c.push(RunnerEvent::Completed {
        output: "answer with source".into(),
    })
    .unwrap();
    assert_eq!(c.finish(Some(0)).unwrap(), "answer with source");
}

#[test]
fn collector_preserves_errors_and_rejects_duplicate_terminal_empty_and_oversized_output() {
    let permit = lease();
    let mut c = RunCollector::new(&permit, 10);
    c.push(RunnerEvent::Started).unwrap();
    assert_eq!(
        c.push(RunnerEvent::Completed {
            output: "字".repeat(11)
        })
        .unwrap_err()
        .code,
        ErrorCode::OutputLimit
    );
    assert!(c.finish(Some(0)).is_err());
    for code in [
        ErrorCode::AuthRequired,
        ErrorCode::HistoryGap,
        ErrorCode::Cancelled,
        ErrorCode::ProviderTimeout,
    ] {
        let permit = lease();
        let mut c = RunCollector::new(&permit, 100);
        c.push(RunnerEvent::Failed { error: code.into() }).unwrap();
        assert_eq!(c.finish(Some(0)).unwrap_err().code, code);
    }
    let permit = lease();
    let mut c = RunCollector::new(&permit, 100);
    c.push(RunnerEvent::Started).unwrap();
    assert!(
        c.push(RunnerEvent::Completed {
            output: "  ".into()
        })
        .is_err()
    );
    let permit = lease();
    let mut c = RunCollector::new(&permit, 100);
    c.push(RunnerEvent::Started).unwrap();
    c.push(RunnerEvent::Completed {
        output: "done".into(),
    })
    .unwrap();
    assert!(
        c.push(RunnerEvent::Completed {
            output: "replacement".into()
        })
        .is_err()
    );
    assert!(c.finish(Some(0)).is_err());
}

#[test]
fn tool_policy_is_an_actual_allowlist_and_does_not_claim_os_isolation() {
    assert_eq!(ToolPolicy::enforcement(), "tool_policy");
    for tool in [
        "context.describe",
        "context.list",
        "context.read",
        "context.search",
        "memory.search",
        "memory.get",
        "code.search",
        "code.read",
        "delegate",
    ] {
        assert!(ToolPolicy::authorize(tool).is_ok());
    }
    for tool in [
        "shell",
        "bash",
        "write",
        "memory.approve",
        "config.set",
        "source.delete",
        "network.fetch",
        "send_message",
        "mem consult",
        "code.read; rm -rf x",
    ] {
        assert_eq!(
            ToolPolicy::authorize(tool).unwrap_err().code,
            ErrorCode::ToolDenied
        );
    }
}

#[test]
fn first_jsonl_frame_negotiates_capabilities_and_failed_handshake_poison_stream() {
    let hello = Envelope {
        schema_version: 1,
        request_id: "request".into(),
        sequence: 0,
        payload: report(),
    };
    let mut bytes = serde_json::to_vec(&hello).unwrap();
    bytes.push(b'\n');
    let mut stream = FrameStream::new("request").unwrap();
    assert_eq!(
        stream
            .handshake(
                &mut std::io::Cursor::new(&bytes),
                &["context.read"],
                &["future"]
            )
            .unwrap()
            .unsupported_optional,
        vec!["future"]
    );
    let mut stream = FrameStream::new("request").unwrap();
    assert_eq!(
        stream
            .handshake(&mut std::io::Cursor::new(&bytes), &["future"], &[])
            .unwrap_err()
            .code,
        ErrorCode::AdapterUnsupported
    );
    let mut next = serde_json::to_vec(&Envelope {
        schema_version: 1,
        request_id: "request".into(),
        sequence: 1,
        payload: RunnerEvent::Started,
    })
    .unwrap();
    next.push(b'\n');
    assert!(
        stream
            .read::<RunnerEvent>(&mut std::io::Cursor::new(next))
            .is_err(),
        "a refused handshake must not admit subsequent events"
    );
    let mut stream = FrameStream::new("request").unwrap();
    assert!(
        stream
            .handshake(&mut std::io::Cursor::new([]), &[], &[])
            .is_err()
    );
    assert!(
        stream
            .read::<CapabilityReport>(&mut std::io::Cursor::new(&bytes))
            .is_err(),
        "EOF during handshake must close the stream"
    );
}
