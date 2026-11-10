use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
pub fn project_argument(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter().map(PathBuf::from).find(|p| {
        p.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mc630"))
    })
}
fn open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}
#[cfg(windows)]
pub fn register(exe: &Path) -> Result<(), String> {
    use windows_sys::Win32::{System::Registry::*, UI::Shell::*};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn set(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        let key = wide(key);
        let name = name.map(wide);
        let value = wide(value);
        let mut handle = std::ptr::null_mut();
        unsafe {
            let result = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            );
            if result != 0 {
                return Err(std::io::Error::from_raw_os_error(result as i32).to_string());
            }
            let result = RegSetValueExW(
                handle,
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            );
            RegCloseKey(handle);
            if result != 0 {
                return Err(std::io::Error::from_raw_os_error(result as i32).to_string());
            }
        }
        Ok(())
    }
    if !exe.is_file() {
        return Err("Application executable not found.".into());
    }
    let icon_dir = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .ok_or("Cannot locate the user application-data folder.")?
        .join("MC630-PLT-Studio")
        .join("Icons");
    std::fs::create_dir_all(&icon_dir).map_err(|e| e.to_string())?;
    let icon = icon_dir.join("project.ico");
    std::fs::write(&icon, include_bytes!("../assets/studio-icon.ico"))
        .map_err(|e| e.to_string())?;
    let icon_location = format!("\"{}\",0", icon.display());
    let progid = "MC630.PLTStudio.Project";
    let base = format!("Software\\Classes\\{progid}");
    let command = open_command(exe);
    set(&base, None, "MC630 Print & Cut Project")?;
    set(&base, Some("FriendlyTypeName"), "MC630 Studio Project")?;
    set(&format!("{base}\\DefaultIcon"), None, &icon_location)?;
    set(&format!("{base}\\shell"), None, "open")?;
    set(&format!("{base}\\shell\\open\\command"), None, &command)?;
    set("Software\\Classes\\.mc630", None, progid)?;
    set(
        "Software\\Classes\\.mc630\\OpenWithProgids",
        Some(progid),
        "",
    )?;
    set(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.mc630\\OpenWithProgids",
        Some(progid),
        "",
    )?;
    let app = format!(
        "Software\\Classes\\Applications\\{}",
        exe.file_name().unwrap().to_string_lossy()
    );
    set(&app, Some("FriendlyAppName"), "MC630 PLT Studio")?;
    set(&format!("{app}\\DefaultIcon"), None, &icon_location)?;
    set(&format!("{app}\\shell\\open\\command"), None, &command)?;
    set(&format!("{app}\\SupportedTypes"), Some(".mc630"), "")?;
    let caps = "Software\\MC630PLTStudio\\Capabilities";
    set(caps, Some("ApplicationName"), "MC630 PLT Studio")?;
    set(caps, Some("ApplicationIcon"), &icon_location)?;
    set(
        caps,
        Some("ApplicationDescription"),
        "Open MC630 print and cut projects",
    )?;
    set(&format!("{caps}\\FileAssociations"), Some(".mc630"), progid)?;
    set(
        "Software\\RegisteredApplications",
        Some("MC630 PLT Studio"),
        caps,
    )?;
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST | SHCNF_FLUSH,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
    Ok(())
}

pub fn configure_default(exe: &Path) -> Result<(), String> {
    register(exe)?;
    webbrowser::open("ms-settings:defaultapps?registeredAppUser=MC630%20PLT%20Studio").map_err(
        |e| format!("Registration completed, but Windows Default apps could not open: {e}"),
    )
}
#[cfg(not(windows))]
pub fn register(_: &Path) -> Result<(), String> {
    Err("File association is supported on Windows only.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_arguments_and_commands_preserve_spaces_and_unicode() {
        let path = PathBuf::from(
            "C:\\Project folder\\\u{1019}\u{102d}\u{102f}\u{1004}\u{103a}\u{1038}.MC630",
        );
        assert_eq!(
            project_argument(vec![
                OsString::from("--ignored"),
                path.clone().into_os_string()
            ]),
            Some(path)
        );
        assert!(project_argument(vec![OsString::from("image.png")]).is_none());
        assert_eq!(
            open_command(Path::new("C:\\Program Files\\Cutter\\studio.exe")),
            "\"C:\\Program Files\\Cutter\\studio.exe\" \"%1\""
        );
    }
}
