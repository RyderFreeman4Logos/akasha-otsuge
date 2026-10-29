//! Opt-in native extension placement and resumable, integrity-checked removal.
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

const SOURCE: &str = include_str!("../../../plugins/pi/index.ts");
const BRIDGE: &[u8] = include_bytes!("../../../plugins/pi/bridge.cjs");
const FILES: [&str; 3] = ["index.ts", "bridge.cjs", "install.json"];

#[derive(serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Installed,
    Removing,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    source_sha256: String,
    bridge_sha256: String,
    phase: Phase,
}

fn directory(path: &Path, create: bool) -> Result<()> {
    if create {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o022 != 0
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(())
}
fn destination(home: &Path, create: bool) -> Result<PathBuf> {
    let home = home.canonicalize()?;
    directory(&home, false)?;
    let root = home.join("extensions");
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
fn verify(path: &Path, resume: bool) -> Result<Receipt> {
    directory(path, false)?;
    let receipt: Receipt = serde_json::from_slice(&read(&path.join(FILES[2]))?)
        .map_err(|_| Error("ADAPTER_INSTALL_CONFLICT"))?;
    if !resume && receipt.phase != Phase::Installed {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    for (name, expected) in FILES[..2]
        .iter()
        .zip([&receipt.source_sha256, &receipt.bridge_sha256])
    {
        let file = path.join(name);
        if resume
            && receipt.phase == Phase::Removing
            && matches!(fs::symlink_metadata(&file), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        if hash(&read(&file)?) != *expected {
            return Err(Error("ADAPTER_INSTALL_CONFLICT"));
        }
    }
    Ok(receipt)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn temporary(path: &Path) -> Result<PathBuf> {
    let mut nonce = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    Ok(path.with_file_name(format!(".akasha-{}", hash(&nonce))))
}

pub(super) fn runner_extension(home: &Path) -> Result<PathBuf> {
    let path = destination(home, false)?;
    verify(&path, false)?;
    let bytes = read(&path.join(FILES[0]))?;
    let installed = std::str::from_utf8(&bytes).map_err(|_| Error("ADAPTER_INSTALL_CONFLICT"))?;
    let defaults = installed
        .lines()
        .find(|line| line.starts_with("const DEFAULTS = "))
        .ok_or(Error("ADAPTER_INSTALL_CONFLICT"))?;
    let _: Value = serde_json::from_str(
        defaults
            .trim_start_matches("const DEFAULTS = ")
            .trim_end_matches(';'),
    )?;
    if SOURCE.replacen("const DEFAULTS = {};", defaults, 1) != installed
        || read(&path.join(FILES[1]))? != BRIDGE
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(path.join(FILES[0]))
}

pub async fn install(binary: &Path, home: &Path, socket: &Path) -> Result<Value> {
    let report = super::probe(binary).await?;
    if report.version.as_deref() != Some(super::SUPPORTED_VERSION) {
        return Err(Error("ADAPTER_UNSUPPORTED"));
    }
    if !socket.is_absolute() {
        return Err(Error("UNSAFE_PATH"));
    }
    let defaults = json!({"socket":socket,"pi_binary":report.executable.ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
        "akasha_binary":std::env::current_exe()?.canonicalize()?});
    let source = SOURCE
        .replacen(
            "const DEFAULTS = {};",
            &format!("const DEFAULTS = {defaults};"),
            1,
        )
        .into_bytes();
    let receipt = serde_json::to_vec(&Receipt {
        source_sha256: hash(&source),
        bridge_sha256: hash(BRIDGE),
        phase: Phase::Installed,
    })?;
    let path = destination(home, true)?;
    if fs::symlink_metadata(&path).is_ok() {
        verify(&path, false)?;
        if read(&path.join(FILES[0]))? != source || read(&path.join(FILES[1]))? != BRIDGE {
            return Err(Error("ADAPTER_INSTALL_CONFLICT"));
        }
    } else {
        let temp = temporary(&path)?;
        fs::DirBuilder::new().mode(0o700).create(&temp)?;
        let result = (|| {
            for (name, bytes) in FILES.into_iter().zip([source.as_slice(), BRIDGE, &receipt]) {
                write_new(&temp.join(name), bytes)?;
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
        json!({"extension_dir":path,"state":"installed","settings_modified":false,"activation":"native_reload_or_restart"}),
    )
}

pub fn uninstall(home: &Path) -> Result<Value> {
    let path = destination(home, false)?;
    if path.try_exists()? {
        directory(&path, false)?;
        if FILES
            .iter()
            .any(|name| fs::symlink_metadata(path.join(name)).is_ok())
        {
            let mut receipt = verify(&path, true)?;
            if receipt.phase == Phase::Installed {
                receipt.phase = Phase::Removing;
                let temp = temporary(&path.join(FILES[2]))?;
                let result = (|| {
                    write_new(&temp, &serde_json::to_vec(&receipt)?)?;
                    fs::rename(&temp, path.join(FILES[2]))?;
                    File::open(&path)?.sync_all()?;
                    Ok::<_, Error>(())
                })();
                if temp.try_exists()? {
                    fs::remove_file(temp)?;
                }
                result?;
            }
            for name in FILES {
                match fs::remove_file(path.join(name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                // Removal intent survives until every owned source unlink is durable.
                File::open(&path)?.sync_all()?;
            }
        }
        match fs::remove_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(e) => return Err(e.into()),
        }
        File::open(path.parent().ok_or(Error("UNSAFE_PATH"))?)?.sync_all()?;
    }
    Ok(
        json!({"extension_dir":path,"state":"removed","settings_modified":false,"directory_retained":path.exists()}),
    )
}
