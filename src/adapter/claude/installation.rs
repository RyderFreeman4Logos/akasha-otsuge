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
const FILES: [&str; 4] = [
    ".claude-plugin/plugin.json",
    "hooks/hooks.json",
    "hooks/caller.js",
    "hooks/config.js",
];

#[derive(Serialize, Deserialize)]
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

fn verified(root: &Path) -> Result<(Receipt, Vec<&'static str>)> {
    directory(
        root,
        &[".claude-plugin", "hooks", RECEIPT],
        &["tsconfig.json"],
    )?;
    directory(&root.join(".claude-plugin"), &["plugin.json"], &["types"])?;
    directory(
        &root.join("hooks"),
        &["hooks.json", "caller.js", "config.js"],
        &[],
    )?;
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
    let receipt: Receipt = serde_json::from_slice(&read(&root.join(RECEIPT))?)?;
    if receipt.schema_version != 1
        || receipt.application != "akasha-otsuge-claude"
        || receipt.files.len() != FILES.len()
        || !receipt.binary.is_absolute()
    {
        return Err(conflict());
    }
    for name in FILES {
        if receipt.files.get(name) != Some(&hash(&read(&root.join(name))?)) {
            return Err(conflict());
        }
    }
    Ok((receipt, preserved))
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
    let (receipt, preserved) = verified(&root)?;
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
    if fs::symlink_metadata(&root).is_ok() {
        if verified(&root)?.0.files != receipt.files {
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
    verified(&stage)?;
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
    let root = destination(path)?;
    let (_, preserved) = verified(&root)?;
    for name in FILES {
        fs::remove_file(root.join(name))?;
    }
    fs::remove_dir(root.join("hooks"))?;
    if !preserved.contains(&".claude-plugin/types") {
        fs::remove_dir(root.join(".claude-plugin"))?;
    }
    fs::remove_file(root.join(RECEIPT))?;
    if preserved.is_empty() {
        fs::remove_dir(&root)?;
    }
    File::open(root.parent().ok_or_else(conflict)?)?.sync_all()?;
    Ok(json!({"plugin_dir":root,"state":"removed","preserved_paths":preserved}))
}
