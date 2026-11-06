use eframe::egui;
use semver::Version;
use serde::Deserialize;
use std::sync::mpsc::{self, Receiver, TryRecvError};

const REPO: &str = "focuseyes1989-debug/mc630-plt-studio";
const ASSET: &str = "MC630-PLT-Studio.exe";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
}

enum ResultState {
    NoRelease,
    Current(Version),
    Available { version: Version, download: String },
}

#[derive(Default)]
pub struct UpdateDialog {
    open: bool,
    pending: Option<Receiver<Result<ResultState, String>>>,
    result: Option<Result<ResultState, String>>,
    browser_error: Option<String>,
}

impl UpdateDialog {
    pub fn start(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.pending.is_some() {
            return;
        }
        self.result = None;
        self.browser_error = None;
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

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.pending {
            let result = match rx.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Update check stopped unexpectedly.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.result = Some(result);
                self.pending = None;
            }
        }
        if !self.open {
            return;
        }
        let mut close = false;
        let mut download = None;
        let dialog = egui::Modal::new(egui::Id::new("studio-update")).show(ctx, |ui| {
            ui.set_width(355.);
            ui.heading("Studio updates");
            ui.label(format!("Installed version: {CURRENT}"));
            ui.separator();
            match &self.result {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking GitHub Releases…");
                    });
                }
                Some(Ok(ResultState::NoRelease)) => {
                    ui.label("No Windows release has been published yet.");
                }
                Some(Ok(ResultState::Current(version))) => {
                    ui.label(format!("You're up to date. Latest release: {version}."));
                }
                Some(Ok(ResultState::Available { version, download: url })) => {
                    ui.strong(format!("Version {version} is available"));
                    ui.label("Download the Windows app, save your projects and close Studio. Replace the current EXE with the downloaded file, then reopen Studio.");
                    if ui.button("Download update").clicked() {
                        download = Some(url.clone());
                    }
                }
                Some(Err(message)) => {
                    ui.label("Couldn't check for updates.");
                    ui.small(message);
                }
            }
            if let Some(error) = &self.browser_error {
                ui.small(error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Release history").clicked() {
                    download = Some(format!("https://github.com/{REPO}/releases"));
                }
                close = ui.button("Close").clicked();
            });
        });
        if let Some(url) = download {
            self.browser_error = webbrowser::open(&url).err().map(|e| e.to_string());
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
    let current = Version::parse(CURRENT).map_err(|_| "Invalid installed version.")?;
    if version.cmp_precedence(&current).is_le() {
        return Ok(ResultState::Current(version));
    }
    if !release.assets.iter().any(|a| a.name == ASSET) {
        return Err("The latest release has no Windows executable yet.".into());
    }
    Ok(ResultState::Available {
        download: format!(
            "https://github.com/{REPO}/releases/download/{}/{ASSET}",
            release.tag_name
        ),
        version,
    })
}

fn check_latest() -> Result<ResultState, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    let mut command = {
        let root = std::env::var_os("SystemRoot").ok_or("Cannot locate Windows system tools.")?;
        std::process::Command::new(
            std::path::PathBuf::from(root)
                .join("System32")
                .join("curl.exe"),
        )
    };
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("curl");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let output = command
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
        .map_err(|_| "Could not start curl for the GitHub update check.")?;
    if !output.status.success() {
        return Err(
            "Could not connect to GitHub. Check your internet connection and try again.".into(),
        );
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
    fn release_check_handles_no_release_current_and_download() {
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
        let release = r#"{"tag_name":"v999.10.0","assets":[{"name":"MC630-PLT-Studio.exe"}]}"#;
        let Ok(ResultState::Available { version, download }) = parse_latest("200", release) else {
            panic!("Expected a newer Windows release");
        };
        assert_eq!(version, Version::new(999, 10, 0));
        assert_eq!(
            download,
            format!("https://github.com/{REPO}/releases/download/v999.10.0/{ASSET}")
        );
        assert!(parse_latest("200", r#"{"tag_name":"v999.10.0","assets":[]}"#).is_err());
        assert!(parse_latest("403", "").is_err());
    }
}
