//! Opt-in plugin placement. Native configuration and user edits are preserved.
use crate::{
    ipc::{Error, Result},
    storage::hash,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const SOURCE: &str = include_str!("../../../plugins/hermes/__init__.py");
const MANIFEST: &[u8] = include_bytes!("../../../plugins/hermes/plugin.yaml");
const FILES: [&str; 3] = ["__init__.py", "plugin.yaml", "install.json"];

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    source_sha256: String,
    manifest_sha256: String,
}

fn directory(path: &Path, create: bool) -> Result<()> {
    if create {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o022 != 0
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(())
}

fn destination(home: &Path, create: bool) -> Result<PathBuf> {
    let home = home.canonicalize()?;
    directory(&home, false)?;
    let root = home.join("plugins");
    directory(&root, create)?;
    Ok(root.join("akasha-otsuge"))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|_| Error("ADAPTER_INSTALL_CONFLICT"))?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o777 != 0o600
        || m.len() > 65536
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(bytes)
}

fn verify(path: &Path) -> Result<()> {
    directory(path, false)?;
    let receipt: Receipt = serde_json::from_slice(&read(&path.join(FILES[2]))?)
        .map_err(|_| Error("ADAPTER_INSTALL_CONFLICT"))?;
    if receipt.source_sha256 != hash(&read(&path.join(FILES[0]))?)
        || receipt.manifest_sha256 != hash(&read(&path.join(FILES[1]))?)
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(())
}

pub async fn install(binary: &Path, home: &Path, socket: &Path) -> Result<Value> {
    let report = super::probe(binary).await?;
    if report.version.as_deref().and_then(|s| s.split('+').next()) != Some(super::SUPPORTED_VERSION)
    {
        return Err(Error("ADAPTER_UNSUPPORTED"));
    }
    if !socket.is_absolute() {
        return Err(Error("UNSAFE_PATH"));
    }
    let defaults = json!({"socket":socket,"hermes_binary":report.executable.ok_or(Error("EXECUTOR_UNAVAILABLE"))?});
    let source = SOURCE
        .replacen("DEFAULTS = {}", &format!("DEFAULTS = {defaults}"), 1)
        .into_bytes();
    let receipt = serde_json::to_vec(&Receipt {
        source_sha256: hash(&source),
        manifest_sha256: hash(MANIFEST),
    })?;
    let path = destination(home, true)?;
    if fs::symlink_metadata(&path).is_ok() {
        verify(&path)?;
        if read(&path.join(FILES[0]))? != source || read(&path.join(FILES[1]))? != MANIFEST {
            return Err(Error("ADAPTER_INSTALL_CONFLICT"));
        }
    } else {
        let mut nonce = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let temp = path.with_file_name(format!(".akasha-{}", hash(&nonce)));
        fs::DirBuilder::new().mode(0o700).create(&temp)?;
        let result = (|| {
            for (name, bytes) in FILES
                .into_iter()
                .zip([source.as_slice(), MANIFEST, &receipt])
            {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(temp.join(name))?;
                file.write_all(bytes)?;
                file.sync_all()?;
            }
            File::open(&temp)?.sync_all()?;
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                &temp,
                rustix::fs::CWD,
                &path,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
            File::open(path.parent().ok_or(Error("UNSAFE_PATH"))?)?.sync_all()?;
            Ok::<_, Error>(())
        })();
        if temp.try_exists()? {
            for name in FILES {
                let file = temp.join(name);
                if file.try_exists()? {
                    fs::remove_file(file)?;
                }
            }
            fs::remove_dir(temp)?;
        }
        result?;
    }
    Ok(
        json!({"plugin_dir":path,"state":"installed","settings_modified":false,"activation":"native_plugin_enable_required"}),
    )
}

pub fn uninstall(home: &Path) -> Result<Value> {
    let path = destination(home, false)?;
    verify(&path)?;
    for name in [FILES[1], FILES[0], FILES[2]] {
        fs::remove_file(path.join(name))?;
    }
    File::open(&path)?.sync_all()?;
    match fs::remove_dir(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
        Err(e) => return Err(e.into()),
    }
    File::open(path.parent().ok_or(Error("UNSAFE_PATH"))?)?.sync_all()?;
    Ok(
        json!({"plugin_dir":path,"state":"removed","settings_modified":false,"directory_retained":path.exists()}),
    )
}
