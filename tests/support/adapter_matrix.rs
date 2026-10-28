use super::*;
use futures::future::BoxFuture;
use std::{collections::VecDeque, time::Duration};

struct Caller {
    host: Host,
    invocation: Invocation,
}
impl CallerAdapter for Caller {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut r = report();
        r.host = self.host;
        Ok(r)
    }
    fn bind<'a>(&'a self, call: &'a str) -> BoxFuture<'a, Result<Invocation>> {
        Box::pin(async move {
            if call == self.invocation.id() {
                Ok(self.invocation.clone())
            } else {
                Err(ErrorCode::BindingUnavailable.into())
            }
        })
    }
}
struct Runner(Host);
struct Handle {
    events: VecDeque<RunnerEvent>,
    spec: RunSpec,
    cancelled: bool,
}
impl AgentRunner for Runner {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut r = report();
        r.host = self.0;
        Ok(r)
    }
    fn start<'a>(
        &'a mut self,
        spec: RunSpec,
        context: &'a mut dyn ContextSource,
    ) -> BoxFuture<'a, Result<Box<dyn RunHandle + 'a>>> {
        Box::pin(async move {
            let ContextResponse::Page(page) = context
                .execute(
                    spec.lease(),
                    ContextRequest::List {
                        cursor: None,
                        limit: 1,
                    },
                )
                .await?
            else {
                return Err(ErrorCode::ProtocolInvalid.into());
            };
            let first = page.events.first().ok_or(ErrorCode::HistoryGap)?;
            let ContextResponse::Text(text) = context
                .execute(
                    spec.lease(),
                    ContextRequest::Read {
                        handle: first.handle.clone(),
                        start: 0,
                        max_bytes: 4096,
                    },
                )
                .await?
            else {
                return Err(ErrorCode::ProtocolInvalid.into());
            };
            let events = VecDeque::from([
                RunnerEvent::Started,
                RunnerEvent::Progress {
                    message: "private trace".into(),
                },
                RunnerEvent::Completed {
                    output: format!("{}: {}", first.reference.event_id, text.text),
                },
            ]);
            Ok(Box::new(Handle {
                events,
                spec,
                cancelled: false,
            }) as Box<dyn RunHandle + 'a>)
        })
    }
}
impl RunHandle for Handle {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<RunnerEvent>>> {
        Box::pin(async move {
            self.spec.check()?;
            Ok(self.events.pop_front())
        })
    }
    fn cancel(&mut self, _reason: CancelReason) -> BoxFuture<'_, Result<CancelAck>> {
        Box::pin(async move {
            self.events.clear();
            self.cancelled = true;
            Ok(CancelAck { acknowledged: true })
        })
    }
    fn exit_status(&mut self) -> BoxFuture<'_, Result<Option<i32>>> {
        Box::pin(async move { Ok(Some(if self.cancelled { 1 } else { 0 })) })
    }
}
fn options() -> RunOptions {
    RunOptions {
        job_id: "job".into(),
        executor_profile: "test-profile".into(),
        instructions_version: "contract-only".into(),
        runner_timeout: Duration::from_secs(20),
        args: ConsultArgs { hint: None },
    }
}

#[tokio::test]
async fn all_sixteen_caller_runner_pairs_use_scoped_context_and_terminal_contract() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let hosts = [Host::ClaudeCode, Host::Codex, Host::Hermes, Host::Pi];
    let mut pairs = 0;
    for (i, caller_host) in hosts.iter().enumerate() {
        let thread = format!("thread-{i}");
        let marker = format!("private-{i}");
        let tip = event(
            &store,
            &thread,
            &format!("source-{i}"),
            &marker,
            None,
            false,
        )
        .await;
        let mut binding = runtime(&f, &store, &thread, tip, &format!("call-{i}"));
        binding.capability.host = *caller_host;
        let inv = Invocation::bind(&store, binding).await.unwrap();
        let caller = Caller {
            host: *caller_host,
            invocation: inv,
        };
        assert_eq!(caller.probe().unwrap().host, *caller_host);
        assert_eq!(
            caller.bind("unknown").await.unwrap_err().code,
            ErrorCode::BindingUnavailable
        );
        for runner_host in hosts {
            let invocation = caller.bind(&format!("call-{i}")).await.unwrap();
            let permit = lease(&invocation);
            let spec = RunSpec::new(
                &invocation,
                options(),
                ReadCapability::issue(&invocation).unwrap(),
                permit,
            )
            .unwrap();
            assert_eq!(spec.config_generation(), 1);
            assert_eq!(spec.origin_invocation_id(), invocation.id());
            let collector_lease = spec.lease_handle();
            let mut context = CapturedContext::new(&store, invocation.clone()).unwrap();
            let mut runner = Runner(runner_host);
            assert_eq!(runner.probe().unwrap().host, runner_host);
            let mut handle = runner.start(spec, &mut context).await.unwrap();
            let mut collector = RunCollector::new(&collector_lease, 4000);
            while let Some(event) = handle.next().await.unwrap() {
                collector.push(event).unwrap();
            }
            assert_eq!(
                collector
                    .finish(handle.exit_status().await.unwrap())
                    .unwrap(),
                format!("source-{i}: {marker}")
            );
            pairs += 1;
        }
    }
    assert_eq!(pairs, 16);
    store.close().await.unwrap();
}

struct BrokenSource(ErrorCode);
impl ContextSource for BrokenSource {
    fn execute<'a>(
        &'a mut self,
        _lease: &'a RunLease,
        _request: ContextRequest,
    ) -> BoxFuture<'a, Result<ContextResponse>> {
        Box::pin(async move { Err(self.0.into()) })
    }
}
#[tokio::test]
async fn external_sources_can_construct_distinct_refusals_and_capabilities_bind_origin() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let tip = event(&store, "root", "tip", "source", None, false).await;
    let inv = Invocation::bind(&store, runtime(&f, &store, "root", tip, "call"))
        .await
        .unwrap();
    for code in [
        ErrorCode::BindingUnavailable,
        ErrorCode::CallerAmbiguous,
        ErrorCode::AdapterUnsupported,
        ErrorCode::HistoryGap,
        ErrorCode::ReadDenied,
        ErrorCode::SourceUnavailable,
        ErrorCode::AuthRequired,
        ErrorCode::ExecutorUnavailable,
    ] {
        let spec = RunSpec::new(
            &inv,
            options(),
            ReadCapability::issue(&inv).unwrap(),
            lease(&inv),
        )
        .unwrap();
        let mut runner = Runner(Host::Pi);
        let mut source = BrokenSource(code);
        match runner.start(spec, &mut source).await {
            Err(e) => assert_eq!(e.code, code),
            Ok(_) => panic!("failure became success"),
        };
    }
    let cap = ReadCapability::issue(&inv).unwrap();
    let permit = lease(&inv);
    assert_ne!(
        cap.transport_token(),
        ReadCapability::issue(&inv).unwrap().transport_token()
    );
    assert!(!format!("{cap:?}").contains(cap.transport_token()));
    assert!(cap.authorize(cap.transport_token(), &permit).is_ok());
    assert_eq!(
        cap.authorize("forged", &permit).unwrap_err().code,
        ErrorCode::ReadDenied
    );
    let wrong = RunBudget::new(
        "different",
        "different",
        std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        BudgetLimits::default(),
        akasha_otsuge::config::deadline::Deadline::start(
            akasha_otsuge::config::deadline::Layer::Workflow,
            Duration::from_secs(10),
            None,
        )
        .unwrap(),
    )
    .unwrap()
    .start()
    .unwrap();
    assert_eq!(
        cap.authorize(cap.transport_token(), &wrong)
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    assert_eq!(
        RunSpec::new(&inv, options(), cap, wrong).unwrap_err().code,
        ErrorCode::ReadDenied
    );
    let spec = RunSpec::new(
        &inv,
        options(),
        ReadCapability::issue(&inv).unwrap(),
        lease(&inv),
    )
    .unwrap();
    let mut source = CapturedContext::new(&store, inv).unwrap();
    let mut runner = Runner(Host::Codex);
    let mut handle = runner.start(spec, &mut source).await.unwrap();
    assert!(
        handle
            .cancel(CancelReason::User)
            .await
            .unwrap()
            .acknowledged
    );
    assert!(handle.next().await.unwrap().is_none());
    assert_eq!(handle.exit_status().await.unwrap(), Some(1));
    drop(handle);
    store.close().await.unwrap();
}

#[tokio::test]
async fn runspec_preserves_earliest_timeout_after_both_clocks_expire() {
    use akasha_otsuge::config::deadline::{Deadline, Layer};
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let tip = event(&store, "root", "tip", "source", None, false).await;
    let inv = Invocation::bind(&store, runtime(&f, &store, "root", tip, "call"))
        .await
        .unwrap();
    tokio::time::pause();
    for (workflow_seconds, runner_seconds, layer, code) in [
        (30, 20, Layer::Runner, ErrorCode::RunnerTimeout),
        (10, 20, Layer::Workflow, ErrorCode::WorkflowTimeout),
        (20, 20, Layer::Workflow, ErrorCode::WorkflowTimeout),
    ] {
        let budget = RunBudget::new(
            inv.id(),
            &inv.fence_hash(),
            std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            BudgetLimits::default(),
            Deadline::start(Layer::Workflow, Duration::from_secs(workflow_seconds), None).unwrap(),
        )
        .unwrap();
        let mut opts = options();
        opts.runner_timeout = Duration::from_secs(runner_seconds);
        let spec = RunSpec::new(
            &inv,
            opts,
            ReadCapability::issue(&inv).unwrap(),
            budget.start().unwrap(),
        )
        .unwrap();
        spec.check().unwrap();
        tokio::time::advance(Duration::from_secs(40)).await;
        assert_eq!(spec.deadline().expired(), Some(layer));
        assert_eq!(spec.check().unwrap_err().code, code);
        budget.cancel();
        assert_eq!(spec.check().unwrap_err().code, ErrorCode::Cancelled);
    }
    tokio::time::resume();
    store.close().await.unwrap();
}
