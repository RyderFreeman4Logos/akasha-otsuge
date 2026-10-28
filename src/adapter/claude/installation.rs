use crate::{
    ipc::{Error, Result},
    storage::hash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const RECEIPT: &str = "akasha-install.json";
const REMOVING: &str = "akasha-uninstall.json";
const FILES: [&str; 4] = [
    ".claude-plugin/plugin.json",
    "hooks/hooks.json",
    "hooks/caller.js",
    "hooks/config.js",
];

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    application: String,
    binary: PathBuf,
    files: BTreeMap<String, String>,
}

fn conflict() -> Error {
    Error("ADAPTER_INSTALL_CONFLICT")
}

fn directory(path: &Path, names: &[&str], optional: &[&str]) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(conflict());
    }
    let actual = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    if names
        .iter()
        .any(|name| !actual.iter().any(|entry| entry == name))
        || actual
            .iter()
            .any(|entry| !names.iter().chain(optional).any(|name| entry == name))
    {
        return Err(conflict());
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(conflict());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(conflict());
    }
    Ok(bytes)
}

fn verified(root: &Path, removing: bool) -> Result<(Receipt, Vec<&'static str>)> {
    let pending = removing && fs::symlink_metadata(root.join(REMOVING)).is_ok();
    if pending {
        directory(
            root,
            &[REMOVING],
            &[".claude-plugin", "hooks", RECEIPT, "tsconfig.json"],
        )?;
    } else {
        directory(
            root,
            &[".claude-plugin", "hooks", RECEIPT],
            &["tsconfig.json"],
        )?;
    }
    for (name, files, optional) in [
        (".claude-plugin", &["plugin.json"][..], &["types"][..]),
        (
            "hooks",
            &["hooks.json", "caller.js", "config.js"][..],
            &[][..],
        ),
    ] {
        if pending {
            match fs::symlink_metadata(root.join(name)) {
                Ok(_) => directory(&root.join(name), &[], &[files, optional].concat())?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
        } else {
            directory(&root.join(name), files, optional)?;
        }
    }
    // Native TUI generates these paths; preserve them, including user edits.
    let mut preserved = Vec::new();
    for (name, is_dir) in [(".claude-plugin/types", true), ("tsconfig.json", false)] {
        match fs::symlink_metadata(root.join(name)) {
            Ok(metadata) => {
                if metadata.uid() != rustix::process::geteuid().as_raw()
                    || (is_dir && !metadata.is_dir())
                    || (!is_dir && !metadata.is_file())
                {
                    return Err(conflict());
                }
                preserved.push(name);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let receipt: Receipt =
        serde_json::from_slice(&read(&root.join(if pending { REMOVING } else { RECEIPT }))?)?;
    if receipt.schema_version != 1
        || receipt.application != "akasha-otsuge-claude"
        || receipt.files.len() != FILES.len()
        || !receipt.binary.is_absolute()
        || FILES.iter().any(|name| !receipt.files.contains_key(*name))
    {
        return Err(conflict());
    }
    if pending
        && fs::symlink_metadata(root.join(RECEIPT)).is_ok()
        && serde_json::from_slice::<Receipt>(&read(&root.join(RECEIPT))?)? != receipt
    {
        return Err(conflict());
    }
    for name in FILES {
        if pending && !root.join(name).try_exists()? {
            // A dangling symlink is a replacement, not a previously removed file.
            if fs::symlink_metadata(root.join(name)).is_ok() {
                return Err(conflict());
            }
            continue;
        }
        if receipt.files.get(name) != Some(&hash(&read(&root.join(name))?)) {
            return Err(conflict());
        }
    }
    Ok((receipt, preserved))
}

fn quarantine(root: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    root.with_file_name(format!(
        ".akasha-uninstall-{}",
        hash(root.as_os_str().as_bytes())
    ))
}

fn rename(from: &Path, to: &Path) -> Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    File::open(to.parent().ok_or_else(conflict)?)?.sync_all()?;
    Ok(())
}

fn destination(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let name = absolute.file_name().ok_or_else(conflict)?;
    Ok(absolute
        .parent()
        .ok_or_else(conflict)?
        .canonicalize()?
        .join(name))
}

pub fn inspect(path: &Path) -> Result<Value> {
    let root = destination(path)?;
    let pending = quarantine(&root);
    let directory = if fs::symlink_metadata(&pending).is_ok() {
        &pending
    } else {
        &root
    };
    if directory == &pending || fs::symlink_metadata(directory.join(REMOVING)).is_ok() {
        verified(directory, true)?;
        return Ok(json!({"plugin_dir":root,"state":"uninstall_pending","recovery_dir":directory}));
    }
    let (receipt, preserved) = verified(&root, false)?;
    Ok(
        json!({"plugin_dir":root,"state":"verified","launch_args":[receipt.binary,"--plugin-dir",root],"preserved_paths":preserved}),
    )
}

struct Staging(Option<PathBuf>);
impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Installs an owned session plugin; native user/project settings are not edited.
pub async fn install(binary: &Path, path: &Path, args: Vec<String>) -> Result<Value> {
    let capability = super::probe(binary).await?;
    if capability.version.as_deref() != Some("2.1.311") {
        return Err(Error("ADAPTER_UNSUPPORTED"));
    }
    let binary = capability.executable.ok_or(Error("EXECUTOR_UNAVAILABLE"))?;
    let bridge = std::env::current_exe()?.canonicalize()?;
    let root = destination(path)?;
    let config = format!(
        "export const bridge={};\nexport const binary={};\nexport const args={};\n",
        serde_json::to_string(&bridge)?,
        serde_json::to_string(&binary)?,
        serde_json::to_string(&args)?
    );
    let files = [
        (FILES[0], json!({"name":"akasha-otsuge","version":env!("CARGO_PKG_VERSION"),"description":"Bound native session capture","license":"Apache-2.0"}).to_string()),
        (FILES[1], "{\"modules\":[\"./caller.js\"]}".into()),
        (FILES[2], include_str!("caller.js").into()),
        (FILES[3], config),
    ];
    let receipt = Receipt {
        schema_version: 1,
        application: "akasha-otsuge-claude".into(),
        binary,
        files: files
            .iter()
            .map(|(name, content)| (name.to_string(), hash(content.as_bytes())))
            .collect(),
    };
    if fs::symlink_metadata(quarantine(&root)).is_ok()
        || fs::symlink_metadata(root.join(REMOVING)).is_ok()
    {
        return Err(Error("ADAPTER_UNINSTALL_PENDING"));
    }
    if fs::symlink_metadata(&root).is_ok() {
        if verified(&root, false)?.0.files != receipt.files {
            return Err(conflict());
        }
        return inspect(&root);
    }
    let parent = root.parent().ok_or_else(conflict)?;
    let mut nonce = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let stage = parent.join(format!(".akasha-claude-{}", hash(&nonce)));
    fs::DirBuilder::new().mode(0o700).create(&stage)?;
    let mut guard = Staging(Some(stage.clone()));
    for name in [".claude-plugin", "hooks"] {
        fs::DirBuilder::new().mode(0o700).create(stage.join(name))?;
    }
    for (name, content) in files
        .into_iter()
        .chain([(RECEIPT, serde_json::to_string(&receipt)?)])
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(stage.join(name))?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    verified(&stage, false)?;
    for name in [".claude-plugin", "hooks", ""] {
        File::open(stage.join(name))?.sync_all()?;
    }
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &stage,
        rustix::fs::CWD,
        &root,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    guard.0 = None;
    File::open(parent)?.sync_all()?;
    inspect(&root)
}

pub fn uninstall(path: &Path) -> Result<Value> {
    uninstall_inner(path, || {})
}

fn uninstall_inner(path: &Path, before_detach: impl FnOnce()) -> Result<Value> {
    let root = destination(path)?;
    let pending = quarantine(&root);
    if fs::symlink_metadata(&pending).is_err() {
        let expected = verified(&root, true)?.0;
        before_detach();
        rename(&root, &pending)?;
        // Verify the detached tree, so an edit/replacement after preflight is retained.
        if !verified(&pending, true).is_ok_and(|(actual, _)| actual == expected) {
            rename(&pending, &root)?;
            return Err(conflict());
        }
    }
    // A crash after deleting the final journal can leave only this empty directory.
    if fs::read_dir(&pending)?.next().is_none() {
        fs::remove_dir(&pending)?;
        File::open(root.parent().ok_or_else(conflict)?)?.sync_all()?;
        return Ok(json!({"plugin_dir":root,"state":"removed","preserved_paths":[]}));
    }
    let (_, preserved) = verified(&pending, true)?;
    if fs::symlink_metadata(pending.join(REMOVING)).is_err() {
        // Linking the immutable receipt publishes the recovery journal atomically.
        fs::hard_link(pending.join(RECEIPT), pending.join(REMOVING))?;
        File::open(&pending)?.sync_all()?;
    }
    for name in FILES.into_iter().chain([RECEIPT]) {
        match fs::remove_file(pending.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    for name in ["hooks", ".claude-plugin"] {
        if name == ".claude-plugin" && preserved.contains(&".claude-plugin/types") {
            continue;
        }
        match fs::remove_dir(pending.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    File::open(&pending)?.sync_all()?;
    let remaining = if preserved.is_empty() {
        &pending
    } else {
        rename(&pending, &root)?;
        &root
    };
    fs::remove_file(remaining.join(REMOVING))?;
    File::open(remaining)?.sync_all()?;
    if preserved.is_empty() {
        fs::remove_dir(&pending)?;
    }
    File::open(root.parent().ok_or_else(conflict)?)?.sync_all()?;
    Ok(json!({"plugin_dir":root,"state":"removed","preserved_paths":preserved}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> (Staging, PathBuf) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "akasha-uninstall-race-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("plugin");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::create_dir(root.join("hooks")).unwrap();
        let mut files = BTreeMap::new();
        for name in FILES {
            fs::write(root.join(name), name).unwrap();
            files.insert(name.into(), hash(name.as_bytes()));
        }
        let receipt = Receipt {
            schema_version: 1,
            application: "akasha-otsuge-claude".into(),
            binary: "/bin/true".into(),
            files,
        };
        fs::write(root.join(RECEIPT), serde_json::to_vec(&receipt).unwrap()).unwrap();
        (Staging(Some(base)), root)
    }

    #[test]
    fn uninstall_retains_a_replacement_between_verification_and_detach() {
        let (_guard, root) = fixture();
        let original = root.with_file_name("original");
        let result = uninstall_inner(&root, || {
            fs::rename(&root, &original).unwrap();
            fs::create_dir(&root).unwrap();
            fs::write(root.join("user-file"), "replacement").unwrap();
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(root.join("user-file")).unwrap(),
            "replacement"
        );
        verified(&original, false).unwrap();
        assert!(!quarantine(&root).exists());
    }

    #[test]
    fn uninstall_retains_an_edit_between_verification_and_detach() {
        let (_guard, root) = fixture();
        assert!(
            uninstall_inner(&root, || fs::write(root.join(FILES[0]), "user edit")
                .unwrap())
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.join(FILES[0])).unwrap(),
            "user edit"
        );
        assert!(root.join(FILES[1]).exists());
        assert!(!quarantine(&root).exists());
    }

    #[test]
    fn interrupted_uninstall_retains_recreated_original_and_edited_remaining_files() {
        let (_guard, root) = fixture();
        let pending = quarantine(&root);
        rename(&root, &pending).unwrap();
        fs::hard_link(pending.join(RECEIPT), pending.join(REMOVING)).unwrap();
        fs::remove_file(pending.join(FILES[0])).unwrap();
        fs::write(pending.join(FILES[1]), "user edit").unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("user-file"), "replacement").unwrap();
        assert!(uninstall(&root).is_err());
        assert_eq!(
            fs::read_to_string(pending.join(FILES[1])).unwrap(),
            "user edit"
        );
        fs::write(pending.join(FILES[1]), FILES[1]).unwrap();
        assert_eq!(inspect(&root).unwrap()["state"], "uninstall_pending");
        assert!(uninstall(&root).is_ok());
        assert_eq!(
            fs::read_to_string(root.join("user-file")).unwrap(),
            "replacement"
        );
        assert!(!pending.exists());
    }

    #[test]
    fn interrupted_uninstall_restores_generated_files_after_destination_conflict() {
        let (_guard, root) = fixture();
        fs::write(root.join("tsconfig.json"), "user config").unwrap();
        let pending = quarantine(&root);
        rename(&root, &pending).unwrap();
        fs::hard_link(pending.join(RECEIPT), pending.join(REMOVING)).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("new-file"), "replacement").unwrap();
        assert!(uninstall(&root).is_err());
        assert_eq!(
            fs::read_to_string(root.join("new-file")).unwrap(),
            "replacement"
        );
        assert_eq!(
            fs::read_to_string(pending.join("tsconfig.json")).unwrap(),
            "user config"
        );
        assert_eq!(inspect(&root).unwrap()["state"], "uninstall_pending");
        fs::rename(&root, root.with_file_name("new-root")).unwrap();
        assert!(uninstall(&root).is_ok());
        assert_eq!(
            fs::read_to_string(root.join("tsconfig.json")).unwrap(),
            "user config"
        );
        assert!(!root.join(REMOVING).exists());
        assert!(!pending.exists());
    }
}
