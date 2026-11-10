use std::sync::Arc;

use surrealkv::{Options, Tree, TreeBuilder};
use tokio::sync::{mpsc, oneshot};

use super::{Result, StorageError, import_publish::Root};

type CloseResult = std::result::Result<(), surrealkv::Error>;

pub(super) struct PinnedTree {
    tree: Arc<Tree>,
    pub(super) root: Root,
    close: mpsc::Sender<oneshot::Sender<CloseResult>>,
}

impl std::ops::Deref for PinnedTree {
    type Target = Tree;

    fn deref(&self) -> &Tree {
        &self.tree
    }
}

impl PinnedTree {
    pub(super) fn open(options: Options, root: &Root) -> Result<Arc<Self>> {
        let (initialized, ready) = std::sync::mpsc::sync_channel(1);
        let (close, mut requests) = mpsc::channel::<oneshot::Sender<CloseResult>>(1);
        let owner = root.clone();
        std::thread::Builder::new()
            .name("akasha-storage".into())
            .spawn(move || {
                // Native shutdown and its tasks must outlive the caller's runtime and cancellation.
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = initialized.send(Err(StorageError::from(error)));
                        return;
                    }
                };
                runtime.block_on(async {
                    let tree = match TreeBuilder::with_options(options).build() {
                        Ok(tree) => Arc::new(tree),
                        Err(error) => {
                            let _ = initialized.send(Err(StorageError::from(error)));
                            return;
                        }
                    };
                    if initialized.send(Ok(tree.clone())).is_ok() {
                        while let Some(reply) = requests.recv().await {
                            let _ = reply.send(tree.close().await);
                        }
                    }
                    let _ = tree.close().await;
                });
                // Dropping the runtime also drains any remaining native blocking work on errors.
                drop(runtime);
                drop(owner);
            })?;
        let tree = ready
            .recv()
            .map_err(|_| StorageError::new("STORAGE_OPEN_FAILED"))??;
        Ok(Arc::new(Self {
            tree,
            root: root.clone(),
            close,
        }))
    }

    pub(super) async fn close(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.close
            .send(reply)
            .await
            .map_err(|_| StorageError::new("STORAGE_CLOSE_FAILED"))?;
        result
            .await
            .map_err(|_| StorageError::new("STORAGE_CLOSE_FAILED"))??;
        Ok(())
    }
}
