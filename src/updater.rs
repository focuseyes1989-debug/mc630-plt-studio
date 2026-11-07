use crate::update_install::{self, Prepared};
use eframe::egui;
use semver::Version;
use serde::Deserialize;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

const REPO: &str = "focuseyes1989-debug/mc630-plt-studio";
const ASSET: &str = "MC630-PLT-Studio.exe";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    #[serde(default)]
    size: u64,
    digest: Option<String>,
}
#[derive(Clone)]
struct Download {
    version: Version,
    tag: String,
    size: u64,
    sha256: Option<String>,
}
enum ResultState {
    NoRelease,
    Current(Version),
    Available(Download),
}

#[derive(Default)]
pub struct UpdateDialog {
    open: bool,
    pending: Option<Receiver<Result<ResultState, String>>>,
    result: Option<Result<ResultState, String>>,
    download_pending: Option<Receiver<Result<Prepared, String>>>,
    downloaded: Arc<AtomicU64>,
    prepared: Option<Prepared>,
    error: Option<String>,
    request_install: bool,
    install_after_close: bool,
}
impl UpdateDialog {
    #[cfg(test)]
    pub fn ready_for_test() -> Self {
        Self {
            prepared: Some(Prepared {
                directory: "missing-update-fixture".into(),
                target: "missing-Studio.exe".into(),
                size: 13,
                sha256: "a".repeat(64),
            }),
            request_install: true,
            ..Default::default()
        }
    }
    pub fn start(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.pending.is_some() || self.download_pending.is_some() || self.prepared.is_some() {
            return;
        }
        self.result = None;
        self.error = None;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check_latest());
            ctx.request_repaint();
        });
        self.pending = Some(rx);
    }
    pub fn menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("Help", |ui| {
            if ui.button("Check for Updates…").clicked() {
                ui.close_menu();
                self.start(ctx);
            }
        });
    }
    pub fn wants_install(&self) -> bool {
        self.request_install
    }
    pub fn begin_install(&mut self) {
        self.request_install = false;
        self.install_after_close = self.prepared.is_some();
        self.open = false;
    }
    pub fn cancel_install(&mut self) {
        if self.install_after_close {
            self.install_after_close = false;
            self.open = true;
        }
    }
    pub fn launch_if_pending(
        &mut self,
        session: &crate::workspace_session::Session,
    ) -> Result<(), String> {
        if !self.install_after_close {
            return Ok(());
        }
        self.install_after_close = false;
        let result = self
            .prepared
            .as_ref()
            .ok_or_else(|| "Update download is missing.".to_string())
            .and_then(|p| p.launch(session));
        if let Err(error) = &result {
            self.error = Some(error.clone());
            self.open = true;
        }
        result
    }
    fn download(&mut self, release: Download, ctx: &egui::Context) {
        self.error = None;
        self.downloaded.store(0, Ordering::Relaxed);
        let progress = self.downloaded.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = download_update(&release, &progress, &ctx);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.download_pending = Some(rx);
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.pending {
            let value = match rx.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Update check stopped unexpectedly.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(value) = value {
                self.result = Some(value);
                self.pending = None;
            }
        }
        if let Some(rx) = &self.download_pending {
            let value = match rx.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Update download stopped unexpectedly.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(value) = value {
                self.download_pending = None;
                match value {
                    Ok(prepared) => {
                        self.prepared = Some(prepared);
                        self.request_install = true;
                        self.open = false;
                        ctx.request_repaint();
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.open = true;
                    }
                }
            }
        }
        if !self.open {
            return;
        }
        let mut close = false;
        let mut download = None;
        let mut history = false;
        let dialog = egui::Modal::new(egui::Id::new("studio-update")).show(ctx, |ui| {
            ui.set_width(380.);
            ui.heading("Studio updates");
            ui.add_space(6.);
            ui.weak(format!("Installed  {CURRENT}"));
            ui.separator();
            if self.prepared.is_some() {
                ui.strong("Update ready");
                ui.label("Studio will save your work, update and reopen.");
                ui.add_space(12.);
                if ui.button("Install and restart").clicked() {
                    self.request_install = true;
                    close = true;
                }
            } else if self.download_pending.is_some() {
                let total = match &self.result {
                    Some(Ok(ResultState::Available(d))) => d.size,
                    _ => 1,
                };
                let bytes = self.downloaded.load(Ordering::Relaxed);
                ui.label("Downloading update…");
                ui.add(
                    egui::ProgressBar::new((bytes as f32 / total.max(1) as f32).min(1.))
                        .show_percentage()
                        .animate(true),
                );
                ui.weak(format!(
                    "{:.1} / {:.1} MB",
                    bytes as f64 / 1_000_000.,
                    total as f64 / 1_000_000.
                ));
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else {
                match &self.result {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking for updates…");
                        });
                    }
                    Some(Ok(ResultState::NoRelease)) => {
                        ui.label("No Windows release is available yet.");
                    }
                    Some(Ok(ResultState::Current(version))) => {
                        ui.strong("You're up to date");
                        ui.label(format!("Latest release  {version}"));
                    }
                    Some(Ok(ResultState::Available(release))) => {
                        ui.strong(format!("Version {} is available", release.version));
                        ui.weak(format!(
                            "Windows · {:.1} MB",
                            release.size as f64 / 1_000_000.
                        ));
                        ui.add_space(10.);
                        ui.label("Download, install and restart automatically.");
                        ui.add_space(10.);
                        if ui.button("Update now").clicked() {
                            download = Some(release.clone());
                        }
                    }
                    Some(Err(error)) => {
                        ui.label("Couldn't check for updates.");
                        ui.small(error);
                    }
                }
            }
            if let Some(error) = &self.error {
                ui.add_space(8.);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.);
            ui.separator();
            ui.horizontal(|ui| {
                history = ui.button("Release history").clicked();
                close |= ui.button("Later").clicked();
            });
        });
        if let Some(release) = download {
            self.download(release, ctx);
        }
        if history {
            self.error = webbrowser::open(&format!("https://github.com/{REPO}/releases"))
                .err()
                .map(|e| e.to_string());
        }
        if close || dialog.should_close() {
            self.open = false;
        }
    }
}

fn parse_latest(status: &str, json: &str) -> Result<ResultState, String> {
    if status == "404" {
        return Ok(ResultState::NoRelease);
    }
    if status != "200" {
        return Err(format!("GitHub returned HTTP {status}. Try again later."));
    }
    let release: Release =
        serde_json::from_str(json).map_err(|_| "Invalid release response from GitHub.")?;
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| "The latest release has an invalid version tag.")?;
    if release.prerelease || release.draft || !version.pre.is_empty() {
        return Err("The latest release is not a stable update.".into());
    }
    let current = Version::parse(CURRENT).map_err(|_| "Invalid installed version.")?;
    if version.cmp_precedence(&current).is_le() {
        return Ok(ResultState::Current(version));
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == ASSET)
        .ok_or("The latest release has no Windows executable yet.")?;
    if !(2..=update_install::MAX_EXE).contains(&asset.size) {
        return Err("The release has an invalid executable size.".into());
    }
    let sha256 = match &asset.digest {
        Some(digest) => Some(valid_digest(
            digest
                .strip_prefix("sha256:")
                .ok_or("Unsupported release checksum.")?,
        )?),
        None => {
            if !release.assets.iter().any(|a| a.name == "SHA256SUMS.txt") {
                return Err("This release has no update checksum.".into());
            }
            None
        }
    };
    Ok(ResultState::Available(Download {
        version,
        tag: release.tag_name,
        size: asset.size,
        sha256,
    }))
}
fn valid_digest(hash: &str) -> Result<String, String> {
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid update checksum.".into());
    }
    Ok(hash.to_ascii_lowercase())
}
fn checksum_file(text: &str) -> Result<String, String> {
    let mut found = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2 && fields[1].trim_start_matches('*') == ASSET {
            if found.is_some() {
                return Err("Ambiguous update checksum.".into());
            }
            found = Some(valid_digest(fields[0])?);
        }
    }
    found.ok_or("No Windows executable checksum was found.".into())
}
fn asset_url(tag: &str, name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{name}")
}

fn download_update(
    release: &Download,
    progress: &AtomicU64,
    ctx: &egui::Context,
) -> Result<Prepared, String> {
    let mut prepared = Prepared::create()?;
    let result = (|| {
        let hash = if let Some(hash) = &release.sha256 {
            hash.clone()
        } else {
            let path = prepared.directory.join("SHA256SUMS.txt");
            fetch_file(
                &asset_url(&release.tag, "SHA256SUMS.txt"),
                &path,
                65_536,
                &AtomicU64::new(0),
                ctx,
            )?;
            checksum_file(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)?
        };
        let path = prepared.directory.join("new.exe");
        fetch_file(
            &asset_url(&release.tag, ASSET),
            &path,
            release.size,
            progress,
            ctx,
        )?;
        update_install::verify(&path, release.size, &hash)?;
        prepared.size = release.size;
        prepared.sha256 = hash;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        for name in ["new.exe", "SHA256SUMS.txt"] {
            let _ = std::fs::remove_file(prepared.directory.join(name));
        }
        let _ = std::fs::remove_dir(&prepared.directory);
        return Err(error);
    }
    let _ = std::fs::remove_file(prepared.directory.join("SHA256SUMS.txt"));
    Ok(prepared)
}
fn fetch_file(
    url: &str,
    path: &Path,
    limit: u64,
    progress: &AtomicU64,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut child = update_install::curl()?
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "180",
            "--max-filesize",
            &limit.to_string(),
            "--user-agent",
            "MC630-PLT-Studio",
            "--output",
        ])
        .arg(path)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    loop {
        if let Ok(meta) = std::fs::metadata(path) {
            progress.store(meta.len(), Ordering::Relaxed);
            ctx.request_repaint();
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err("Download failed. Check your connection and try again; Studio has not been changed.".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(150)),
        }
    }
}
fn check_latest() -> Result<ResultState, String> {
    let output = update_install::curl()?
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "12",
            "--max-filesize",
            "1048576",
            "--user-agent",
            "MC630-PLT-Studio",
            "--header",
            "Accept: application/vnd.github+json",
            "--write-out",
            "\n%{http_code}",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .map_err(|_| "Could not start the update check.")?;
    if !output.status.success() {
        return Err("Could not connect to GitHub. Check your connection and try again.".into());
    }
    let response =
        std::str::from_utf8(&output.stdout).map_err(|_| "Invalid response from GitHub.")?;
    let (json, status) = response
        .rsplit_once('\n')
        .ok_or("Missing HTTP status from GitHub.")?;
    parse_latest(status, json)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_checks_require_stable_version_executable_and_integrity() {
        assert!(matches!(
            parse_latest("404", ""),
            Ok(ResultState::NoRelease)
        ));
        assert!(matches!(
            parse_latest(
                "200",
                &format!(r#"{{"tag_name":"v{CURRENT}","assets":[]}}"#)
            ),
            Ok(ResultState::Current(_))
        ));
        let json = format!(
            r#"{{"tag_name":"v999.10.0","assets":[{{"name":"MC630-PLT-Studio.exe","size":12345,"digest":"sha256:{}"}}]}}"#,
            "a".repeat(64)
        );
        let Ok(ResultState::Available(release)) = parse_latest("200", &json) else {
            panic!("Expected update");
        };
        assert_eq!(release.version, Version::new(999, 10, 0));
        assert_eq!(
            asset_url(&release.tag, ASSET),
            format!("https://github.com/{REPO}/releases/download/v999.10.0/{ASSET}")
        );
        assert!(parse_latest("200", &json.replace("v999.10.0", "v999.10.0-beta.1")).is_err());
        assert!(parse_latest("200", &json.replace("12345", "0")).is_err());
        assert!(parse_latest("200", r#"{"tag_name":"v999.10.0","assets":[]}"#).is_err());
        assert!(parse_latest("403", "").is_err());
    }
    #[test]
    fn checksum_parser_selects_exact_asset_and_rejects_corruption() {
        let hash = "ab".repeat(32);
        assert_eq!(
            checksum_file(&format!("{}  MC630-PLT-Studio.exe\n", "ab".repeat(32))).unwrap(),
            hash
        );
        assert!(checksum_file("bad  MC630-PLT-Studio.exe").is_err());
        assert!(checksum_file(&format!("{hash}  other.exe")).is_err());
        assert!(checksum_file(&format!("{hash}  {ASSET}\n{hash}  {ASSET}")).is_err());
        let json = r#"{"tag_name":"v999.10.0","assets":[{"name":"MC630-PLT-Studio.exe","size":13},{"name":"SHA256SUMS.txt","size":88}]}"#;
        assert!(matches!(
            parse_latest("200", json),
            Ok(ResultState::Available(_))
        ));
    }
}
