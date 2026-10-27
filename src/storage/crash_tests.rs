use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::*;
use crate::config::{Paths, resolve};

pub(super) fn checkpoint(phase: &str) {
    if std::env::var("AKASHA_TEST_CRASH_PHASE").as_deref() != Ok(phase) {
        return;
    }
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    std::fs::write(root.join("reached"), phase).unwrap();
    loop {
        std::thread::park();
    }
}

fn config(root: &Path) -> ResolvedConfig {
    resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: root.join("data"),
            socket_path: root.join("daemon.sock"),
        },
    )
    .unwrap()
}

fn request(id: &str) -> CommitRequest {
    CommitRequest {
        request_id: id.into(),
        config_generation: 1,
        policy_revision: Some("policy-1".into()),
        changes: [
            RecordKind::Evidence,
            RecordKind::Memory,
            RecordKind::State,
            RecordKind::Job,
        ]
        .into_iter()
        .map(|kind| Change {
            key: RecordKey {
                kind,
                id: id.into(),
            },
            expected_revision: None,
            content: "real atomic content\n".repeat(1024),
        })
        .collect(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
#[ignore = "launched only by the crash supervisor with an isolated database"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        let receipt = store.commit(&request("crash")).await.unwrap();
        assert_eq!(receipt.commit_seq, 2);
        checkpoint("after-reply");
        panic!("requested crash checkpoint was not reached");
    });
}

struct OwnedFixture(PathBuf);

impl Drop for OwnedFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn killed_transactions_recover_whole_batches_and_retry_exactly_once() {
    for phase in [
        "before-stage",
        "partial-stage",
        "before-commit",
        "commit-pending",
        "after-commit",
        "after-reply",
    ] {
        let root =
            std::env::temp_dir().join(format!("akasha-crash-{}-{phase}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        let config = config(&root);
        runtime().block_on(async {
            let store = Storage::open(&config).await.unwrap();
            store.commit(&request("baseline")).await.unwrap();
            store.close().await.unwrap();
        });
        let log = std::fs::File::create(root.join("child.log")).unwrap();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::crash_tests::crash_child",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("AKASHA_TEST_CRASH_ROOT", &root)
                .env("AKASHA_TEST_CRASH_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.join("reached").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before {phase}: {}",
                std::fs::read_to_string(root.join("child.log")).unwrap()
            );
            assert!(Instant::now() < deadline, "child did not reach {phase}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("reached")).unwrap(),
            phase
        );
        child.0.kill().unwrap();
        let status = child.0.wait().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(9));
        }
        assert!(!status.success());
        runtime().block_on(async {
            let store = Storage::open(&config).await.unwrap();
            let receipt = store.receipt("crash").unwrap();
            let seq = store.sequence().unwrap();
            let committed = receipt.is_some();
            assert_eq!(seq, if committed { 2 } else { 1 }, "{phase}");
            if phase.starts_with("after-") {
                assert!(committed, "lost acknowledged commit at {phase}");
            }
            if ["before-stage", "partial-stage", "before-commit"].contains(&phase) {
                assert!(!committed, "{phase}");
            }
            assert_eq!(store.outbox(2).unwrap().is_some(), committed);
            for change in &request("crash").changes {
                let current = store.current(&change.key).unwrap();
                assert_eq!(current.is_some(), committed, "{phase}");
                assert_eq!(store.revision(&change.key, 2).unwrap(), current);
                if let Some(revision) = current {
                    assert_eq!(revision.content, change.content);
                    assert_eq!(revision.content_sha256, hash(change.content.as_bytes()));
                    assert_eq!(revision.previous, None);
                }
            }
            let recovered = store.commit(&request("crash")).await.unwrap();
            if let Some(receipt) = receipt {
                assert_eq!(recovered, receipt);
            }
            assert_eq!(recovered.commit_seq, 2);
            assert_eq!(store.commit(&request("crash")).await.unwrap(), recovered);
            assert_eq!(store.sequence().unwrap(), 2);
            assert!(store.outbox(3).unwrap().is_none());
            for change in &request("baseline").changes {
                assert_eq!(
                    store.current(&change.key).unwrap().unwrap().content,
                    change.content
                );
            }
            store.close().await.unwrap();
            println!(
                "crash checkpoint={phase} recovered_seq={seq} retry_seq=2 all_records_atomic=true"
            );
        });
    }
}
