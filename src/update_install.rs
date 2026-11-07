//! The old executable runs a detached copy of itself to replace the locked app.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

pub const MAX_EXE: u64 = 200_000_000;

pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub fn curl() -> Result<Command, String> {
    #[cfg(windows)]
    let mut command = Command::new(
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("Cannot locate Windows system tools.")?)
            .join("System32/curl.exe"),
    );
    #[cfg(not(windows))]
    let mut command = Command::new("curl");
    hidden(&mut command);
    Ok(command)
}

pub fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn verify(path: &Path, expected_size: u64, sha256: &str) -> Result<(), String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size != expected_size || size < 2 || size > MAX_EXE {
        return Err("The downloaded update is incomplete. Try again.".into());
    }
    let mut header = [0; 2];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| e.to_string())?;
    if header != *b"MZ" || digest(path)? != sha256 {
        return Err(
            "The update failed SHA-256 verification. The installed app has not been changed."
                .into(),
        );
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct Prepared {
    pub directory: PathBuf,
    pub target: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Plan {
    update: Prepared,
    parent_pid: u32,
}

impl Prepared {
    pub fn create() -> Result<Self, String> {
        let target = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = target
            .parent()
            .ok_or("Cannot locate Studio folder.")?
            .join(format!(".mc630-update-{}-{stamp}", std::process::id()));
        fs::create_dir(&directory).map_err(|_| {
            "Studio's folder is not writable. Move the app to a writable folder and try again."
                .to_string()
        })?;
        Ok(Self {
            directory,
            target,
            size: 0,
            sha256: String::new(),
        })
    }

    fn validate_paths(&self) -> Result<(), String> {
        let dir = fs::canonicalize(&self.directory).map_err(|e| e.to_string())?;
        let parent = self.target.parent().ok_or("Invalid update destination.")?;
        let parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
        if !self.target.is_absolute()
            || dir.parent() != Some(parent.as_path())
            || !dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".mc630-update-"))
            || self
                .target
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("exe"))
        {
            return Err("Invalid update destination.".into());
        }
        Ok(())
    }

    pub fn launch(&self, session: &crate::workspace_session::Session) -> Result<(), String> {
        self.validate_paths()?;
        verify(&self.directory.join("new.exe"), self.size, &self.sha256)?;
        let helper = self.directory.join("helper.exe");
        fs::copy(&self.target, &helper)
            .map_err(|e| format!("Cannot prepare update helper: {e}"))?;
        fs::write(
            self.directory.join("resume.json"),
            serde_json::to_vec(session).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let plan = Plan {
            update: Prepared {
                directory: self.directory.clone(),
                target: self.target.clone(),
                size: self.size,
                sha256: self.sha256.clone(),
            },
            parent_pid: std::process::id(),
        };
        let plan_path = self.directory.join("plan.json");
        fs::write(
            &plan_path,
            serde_json::to_vec(&plan).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        hidden(Command::new(helper).arg("--apply-update").arg(plan_path))
            .spawn()
            .map_err(|e| format!("Cannot start update helper: {e}"))?;
        Ok(())
    }
}

#[cfg(windows)]
fn wait_for_parent(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    unsafe {
        let process = OpenProcess(0x00100000, 0, pid); // SYNCHRONIZE only
        if process.is_null() {
            return if std::io::Error::last_os_error().raw_os_error() == Some(87) {
                Ok(())
            } else {
                Err("Cannot wait for Studio to close.".into())
            };
        }
        let result = WaitForSingleObject(process, 120_000);
        CloseHandle(process);
        if result != 0 {
            return Err("Studio did not close. The update was not installed.".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn wait_for_parent(_: u32) -> Result<(), String> {
    Err("Automatic updates require Windows.".into())
}

/// Roll back both replacement errors and immediate process launch errors.
fn replace_and_restart(
    update: &Prepared,
    mut restart: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    update.validate_paths()?;
    let staged = update.directory.join("new.exe");
    let backup = update.directory.join("previous.exe");
    verify(&staged, update.size, &update.sha256)?;
    if backup.exists() {
        return Err("An earlier update attempt already has a backup.".into());
    }
    let mut moved = false;
    let mut last = String::new();
    for _ in 0..80 {
        match fs::rename(&update.target, &backup) {
            Ok(()) => {
                moved = true;
                break;
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
    if !moved {
        return Err(format!(
            "Cannot replace Studio (another instance may still be open): {last}"
        ));
    }
    if let Err(error) = fs::rename(&staged, &update.target) {
        fs::rename(&backup, &update.target).map_err(|rollback| format!("Update failed: {error}. Original executable remains at {}. Restore failed: {rollback}", backup.display()))?;
        return Err(format!(
            "Update failed; the previous version was restored: {error}"
        ));
    }
    if let Err(error) = restart(&update.target) {
        fs::rename(&update.target, &staged).map_err(|e| {
            format!(
                "Cannot roll back after restart failed: {e}. Backup: {}",
                backup.display()
            )
        })?;
        fs::rename(&backup, &update.target).map_err(|e| {
            format!(
                "Cannot restore the backup: {e}. Backup: {}",
                backup.display()
            )
        })?;
        return Err(format!(
            "Cannot restart the update; the previous version was restored: {error}"
        ));
    }
    Ok(())
}

fn restart(target: &Path, directory: &Path) -> Result<(), String> {
    Command::new(target)
        .arg("--resume-update")
        .arg(directory.join("resume.json"))
        .current_dir(target.parent().ok_or("Invalid Studio folder.")?)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Returns true only for the helper invocation, before any GUI is created.
pub fn run_helper() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|a| a != "--apply-update") {
        return false;
    }
    let result = (|| {
        let path = args.get(2).ok_or("Missing update plan.")?;
        let plan: Plan = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        plan.update.validate_paths()?;
        let helper = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|e| e.to_string())?;
        let dir = fs::canonicalize(&plan.update.directory).map_err(|e| e.to_string())?;
        if helper.parent() != Some(dir.as_path()) {
            return Err("The update helper is in the wrong folder.".into());
        }
        wait_for_parent(plan.parent_pid)?;
        if let Err(error) = replace_and_restart(&plan.update, |target| {
            restart(target, &plan.update.directory)
        }) {
            let _ = fs::write(plan.update.directory.join("update-error.txt"), &error);
            // The previous executable is still present (or has been restored).
            if plan.update.target.is_file() {
                let _ = restart(&plan.update.target, &plan.update.directory);
            }
            return Err(error);
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        rfd::MessageDialog::new()
            .set_title("Studio update failed")
            .set_description(error)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    true
}

pub struct Resume {
    pub session: crate::workspace_session::Session,
    pub cleanup_directory: Option<PathBuf>,
}

pub fn resume() -> Option<Resume> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1)? != "--resume-update" {
        return None;
    }
    let path = PathBuf::from(args.get(2)?);
    let directory = path.parent()?.to_owned();
    let executable = std::env::current_exe().ok()?;
    let plan: Plan = serde_json::from_slice(&fs::read(directory.join("plan.json")).ok()?).ok()?;
    if plan.update.validate_paths().is_err()
        || fs::canonicalize(executable).ok()? != fs::canonicalize(&plan.update.target).ok()?
        || fs::canonicalize(&directory).ok()? != fs::canonicalize(&plan.update.directory).ok()?
        || path.file_name()? != "resume.json"
    {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    if bytes.len() > 1_000_000 {
        return None;
    }
    let session = serde_json::from_slice::<crate::workspace_session::Session>(&bytes)
        .ok()?
        .normalized();
    let cleanup_directory = if directory.join("update-error.txt").exists() {
        None
    } else {
        Some(directory)
    };
    Some(Resume {
        session,
        cleanup_directory,
    })
}

/// Called only after the updated GUI has opened successfully.
pub fn cleanup_success(directory: PathBuf) {
    std::thread::spawn(move || {
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if fs::remove_file(directory.join("helper.exe")).is_ok() {
                break;
            }
        }
        for name in ["previous.exe", "new.exe", "resume.json", "plan.json"] {
            let _ = fs::remove_file(directory.join(name));
        }
        let _ = fs::remove_dir(directory);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Prepared {
        let root = std::env::temp_dir().join(format!(
            "mc630-install-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let target = root.join("Studio.exe");
        fs::write(&target, b"MZold version").unwrap();
        let directory = root.join(".mc630-update-test");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("new.exe"), b"MZnew version").unwrap();
        Prepared {
            sha256: digest(&directory.join("new.exe")).unwrap(),
            size: 13,
            directory,
            target,
        }
    }
    fn clean(update: &Prepared) {
        fs::remove_dir_all(update.target.parent().unwrap()).unwrap();
    }
    #[test]
    fn replacement_uses_verified_file_and_retains_backup() {
        let update = fixture();
        replace_and_restart(&update, |path| {
            assert_eq!(fs::read(path).unwrap(), b"MZnew version");
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fs::read(update.directory.join("previous.exe")).unwrap(),
            b"MZold version"
        );
        clean(&update);
    }
    #[test]
    fn failed_restart_restores_the_previous_executable() {
        let update = fixture();
        assert!(replace_and_restart(&update, |_| Err("test launch error".into())).is_err());
        assert_eq!(fs::read(&update.target).unwrap(), b"MZold version");
        clean(&update);
    }
    #[test]
    fn corrupt_download_never_changes_installed_app() {
        let update = fixture();
        fs::write(update.directory.join("new.exe"), b"MZbad version").unwrap();
        assert!(
            replace_and_restart(&update, |_| panic!("Must not launch corrupt update")).is_err()
        );
        assert_eq!(fs::read(&update.target).unwrap(), b"MZold version");
        clean(&update);
    }
}
