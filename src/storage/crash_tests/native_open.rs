use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, mpsc};

type Hold = (mpsc::Sender<()>, mpsc::Receiver<()>);
static HOLDS: LazyLock<Mutex<BTreeMap<PathBuf, Hold>>> = LazyLock::new(Mutex::default);

pub(crate) struct Gate {
    path: PathBuf,
    pub reached: mpsc::Receiver<()>,
    pub release: Option<mpsc::Sender<()>>,
}

impl Gate {
    pub(crate) fn new(path: &Path) -> Self {
        let path = path.canonicalize().unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        assert!(
            HOLDS
                .lock()
                .unwrap()
                .insert(path.clone(), (entered, resume))
                .is_none()
        );
        Self {
            path,
            reached,
            release: Some(release),
        }
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        HOLDS.lock().unwrap().remove(&self.path);
        self.release.take();
    }
}

pub(crate) fn checkpoint(path: &Path) {
    let Ok(path) = path.canonicalize() else {
        return;
    };
    let hold = HOLDS.lock().unwrap().remove(&path);
    if let Some((reached, resume)) = hold {
        let _ = reached.send(());
        let _ = resume.recv();
    }
}
