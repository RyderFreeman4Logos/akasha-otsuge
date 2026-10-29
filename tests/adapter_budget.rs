use akasha_otsuge::adapter::*;
use akasha_otsuge::config::deadline::{Deadline, Layer};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

fn budget(slots: Arc<Semaphore>, limits: BudgetLimits) -> RunBudget {
    RunBudget::new(
        "origin",
        "fence",
        slots,
        limits,
        Deadline::start(Layer::Workflow, Duration::from_secs(100), None).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn descendants_retry_and_output_share_one_budget_and_original_origin() {
    let b = budget(
        Arc::new(Semaphore::new(3)),
        BudgetLimits {
            reads: 2,
            read_bytes: 8,
            output_bytes: 8,
            ..BudgetLimits::default()
        },
    );
    let root = b.start().unwrap();
    let child = root.delegate().unwrap();
    assert_eq!(child.origin(), "origin");
    assert_eq!(child.fence(), "fence");
    assert_eq!(child.depth(), 1);
    assert_eq!(
        child.delegate().unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    root.charge_read(4).unwrap();
    child.charge_read(4).unwrap();
    assert_eq!(
        root.charge_read(1).unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    root.charge_output(5).unwrap();
    child.charge_output(3).unwrap();
    assert_eq!(
        child.charge_output(1).unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    drop(child);
    drop(root);
    drop(b.start().unwrap());
    assert_eq!(
        b.start().unwrap_err().code,
        ErrorCode::BudgetExhausted,
        "root retry is the third and last start"
    );
    assert_eq!(
        b.usage().unwrap(),
        BudgetUsage {
            starts: 3,
            reads: 2,
            read_bytes: 8,
            output_bytes: 8,
            tokens: 0,
        }
    );
}

#[tokio::test]
async fn two_parents_never_wait_for_child_slots_and_one_worker_still_works() {
    let slots = Arc::new(Semaphore::new(2));
    let left = budget(slots.clone(), BudgetLimits::default());
    let right = budget(slots.clone(), BudgetLimits::default());
    let a = left.start().unwrap();
    let b = right.start().unwrap();
    let (x, y) = tokio::join!(async { a.delegate() }, async { b.delegate() });
    assert_eq!(x.unwrap_err().code, ErrorCode::DelegationUnavailable);
    assert_eq!(y.unwrap_err().code, ErrorCode::DelegationUnavailable);
    assert_eq!(left.usage().unwrap().starts, 1);
    assert_eq!(right.usage().unwrap().starts, 1);
    a.charge_read(1).unwrap();
    drop(b);
    drop(a);
    assert_eq!(slots.available_permits(), 2);
    let one = budget(Arc::new(Semaphore::new(1)), BudgetLimits::default());
    let only = one.start().unwrap();
    assert_eq!(
        only.delegate().unwrap_err().code,
        ErrorCode::DelegationUnavailable
    );
    only.charge_read(1).unwrap();
    only.charge_output(1).unwrap();
}

#[tokio::test(start_paused = true)]
async fn cancel_and_original_deadline_apply_to_every_descendant() {
    let b = budget(Arc::new(Semaphore::new(3)), BudgetLimits::default());
    let root = b.start().unwrap();
    let child = root.delegate().unwrap();
    tokio::time::advance(Duration::from_secs(100)).await;
    assert_eq!(
        child.charge_read(1).unwrap_err().code,
        ErrorCode::WorkflowTimeout
    );
    assert_eq!(b.start().unwrap_err().code, ErrorCode::WorkflowTimeout);
    let b = budget(Arc::new(Semaphore::new(2)), BudgetLimits::default());
    let root = b.start().unwrap();
    let child = root.delegate().unwrap();
    b.cancel();
    assert_eq!(
        child.charge_output(1).unwrap_err().code,
        ErrorCode::Cancelled
    );
    assert_eq!(root.charge_read(1).unwrap_err().code, ErrorCode::Cancelled);
}

#[test]
fn reported_tokens_exhaust_one_shared_budget_and_stop_every_descendant() {
    let b = budget(Arc::new(Semaphore::new(2)), BudgetLimits::default());
    let root = b.start().unwrap();
    let child = root.delegate().unwrap();
    let mut parent_output = RunCollector::new(&root, 100);
    let mut child_output = RunCollector::new(&child, 100);
    parent_output.push(RunnerEvent::Started).unwrap();
    child_output.push(RunnerEvent::Started).unwrap();
    parent_output
        .push(RunnerEvent::Usage {
            input_tokens: 100_000,
            output_tokens: 0,
        })
        .unwrap();
    assert_eq!(
        child_output
            .push(RunnerEvent::Usage {
                input_tokens: 100_000,
                output_tokens: 0,
            })
            .unwrap_err()
            .code,
        ErrorCode::BudgetExhausted
    );
    assert_eq!(root.check().unwrap_err().code, ErrorCode::BudgetExhausted);
    assert_eq!(child.check().unwrap_err().code, ErrorCode::BudgetExhausted);
    assert_eq!(
        root.delegate().unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    assert!(
        parent_output
            .push(RunnerEvent::Completed {
                output: "over budget".into()
            })
            .is_err()
    );
    assert_eq!(b.usage().unwrap().tokens, 200_000);
    assert_eq!(
        root.charge_tokens(2, 3).unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    assert_eq!(
        b.usage().unwrap().tokens,
        200_005,
        "retain late observed in-flight usage"
    );
    drop(parent_output);
    drop(child_output);
    drop(root);
    drop(child);
    assert_eq!(b.start().unwrap_err().code, ErrorCode::BudgetExhausted);
}

#[test]
fn token_accounting_preserves_exact_limit_and_rejects_overflow() {
    let b = budget(
        Arc::new(Semaphore::new(1)),
        BudgetLimits {
            tokens: u64::MAX,
            ..BudgetLimits::default()
        },
    );
    let root = b.start().unwrap();
    root.charge_tokens(u64::MAX - 3, 3).unwrap();
    assert_eq!(b.usage().unwrap().tokens, u64::MAX);
    assert_eq!(
        root.charge_tokens(1, 0).unwrap_err().code,
        ErrorCode::BudgetExhausted
    );
    assert_eq!(root.check().unwrap_err().code, ErrorCode::BudgetExhausted);
}

#[test]
fn cumulative_usage_does_not_double_bill_native_and_collector_reports() {
    let b = budget(Arc::new(Semaphore::new(1)), BudgetLimits::default());
    let root = b.start().unwrap();
    let mut collector = RunCollector::new(&root, 100);
    collector.push(RunnerEvent::Started).unwrap();
    for (input_tokens, output_tokens) in [(10, 2), (10, 2), (11, 3)] {
        collector
            .push(RunnerEvent::Usage {
                input_tokens,
                output_tokens,
            })
            .unwrap();
    }
    assert_eq!(b.usage().unwrap().tokens, 14);
    assert_eq!(
        collector
            .push(RunnerEvent::Usage {
                input_tokens: 9,
                output_tokens: 3
            })
            .unwrap_err()
            .code,
        ErrorCode::ProtocolInvalid
    );
}
