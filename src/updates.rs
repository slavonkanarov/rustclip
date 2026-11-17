use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

pub const VERSION: &str = match option_env!("RUSTCLIP_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const REPOSITORY: &str = "https://api.github.com/repos/slavonkanarov/rustclip/releases";
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub prerelease: bool,
    draft: bool,
    assets: Vec<Asset>,
}

impl Release {
    pub fn version(&self) -> Option<Version> {
        Version::parse(self.tag_name.trim_start_matches('v')).ok()
    }

    pub fn newer(&self) -> bool {
        self.version()
            .zip(Version::parse(VERSION).ok())
            .is_some_and(|(a, b)| a > b)
    }

    fn asset(&self, installed: bool) -> Option<&Asset> {
        self.assets.iter().find(|a| {
            if installed {
                a.name == format!("RustClip-{}-x64.msi", self.tag_name.trim_start_matches('v'))
            } else {
                a.name == "rustclip.exe"
            }
        })
    }
}

pub enum Event {
    Releases(Result<Vec<Release>, String>),
    Progress(u64, u64),
    Downloaded(Result<Downloaded, String>),
}

pub struct Downloaded {
    pub release: Release,
    pub package: PathBuf,
    pub sha256: String,
    recovery: Option<Recovery>,
}

#[derive(Serialize)]
struct Recovery {
    package: PathBuf,
    sha256: String,
}

pub struct Updates {
    pub releases: Vec<Release>,
    pub selected: usize,
    pub busy: bool,
    pub progress: Option<(u64, u64)>,
    pub downloaded: Option<Downloaded>,
    pub message: String,
    pub install_requested: bool,
    pub automatic: bool,
    pub last_check: Option<Instant>,
    receiver: Option<Receiver<Event>>,
    cancelled: Arc<AtomicBool>,
    pub installed: Option<String>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            releases: Vec::new(),
            selected: 0,
            busy: false,
            progress: None,
            downloaded: None,
            message: String::new(),
            install_requested: false,
            automatic: false,
            last_check: None,
            receiver: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            installed: installed_product(),
        }
    }
}

fn client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent(concat!("RustClip/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
}

fn list_releases(cancelled: &AtomicBool) -> Result<Vec<Release>, Box<dyn std::error::Error>> {
    let client = client()?;
    let mut releases = Vec::new();
    // Paginate so older versions remain accessible as the project grows.
    for page in 1..=100 {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Operation cancelled".into());
        }
        let response = client
            .get(REPOSITORY)
            .query(&[("per_page", 100), ("page", page)])
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(30))
            .send()?
            .error_for_status()?;
        let mut bytes = Vec::new();
        response.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Release list exceeds size limit".into());
        }
        let batch: Vec<Release> = serde_json::from_slice(&bytes)?;
        let done = batch.len() < 100;
        releases.extend(
            batch
                .into_iter()
                .filter(|r| !r.draft && r.version().is_some()),
        );
        if done {
            break;
        }
    }
    releases.sort_by_key(|r| std::cmp::Reverse(r.version()));
    Ok(releases)
}

impl Updates {
    pub fn check(&mut self, ctx: &eframe::egui::Context, automatic: bool) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = "Checking releases…".into();
        self.last_check = Some(Instant::now());
        self.automatic = automatic;
        self.install_requested = false;
        self.cancelled.store(false, Ordering::Relaxed);
        let cancelled = self.cancelled.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = list_releases(&cancelled)
                .and_then(|releases| {
                    if cancelled.load(Ordering::Relaxed) {
                        Err("Operation cancelled".into())
                    } else {
                        Ok(releases)
                    }
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Releases(result));
            ctx.request_repaint();
        });
    }

    pub fn download(&mut self, release: Release, ctx: &eframe::egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.downloaded = None;
        self.progress = None;
        self.message = "Downloading…".into();
        let installed = self.installed.is_some();
        self.cancelled.store(false, Ordering::Relaxed);
        let cancelled = self.cancelled.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress = |done, total| {
                let _ = tx.send(Event::Progress(done, total));
                ctx.request_repaint();
            };
            let result = (|| -> Result<Downloaded, Box<dyn std::error::Error>> {
                let current_version = Version::parse(VERSION).ok();
                let recovery = if installed
                    && release.version().zip(current_version.as_ref()).is_some_and(|(a, b)| a < *b)
                {
                    let current = list_releases(&cancelled)?
                        .into_iter()
                        .find(|r| r.version() == current_version)
                        .ok_or("Cannot downgrade: current MSI is not available for recovery in GitHub Releases")?;
                    let (package, sha256) = download(&current, true, &cancelled, progress)?;
                    Some(Recovery { package, sha256 })
                } else {
                    None
                };
                let (package, sha256) = download(&release, installed, &cancelled, progress)?;
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Operation cancelled".into());
                }
                Ok(Downloaded { release, package, sha256, recovery })
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(Event::Downloaded(result));
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> Option<Event> {
        self.receiver.as_ref()?.try_recv().ok()
    }

    pub fn cancel(&mut self) {
        self.automatic = false;
        self.install_requested = false;
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn available(&self, prereleases: bool) -> Option<&Release> {
        self.releases
            .iter()
            .find(|r| r.newer() && (!r.prerelease || prereleases))
    }
}

fn trusted_asset(url: &str) -> bool {
    url.starts_with("https://github.com/slavonkanarov/rustclip/releases/download/")
}

fn checksum(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start().trim_start_matches('*') == name
            && hash.len() == 64
            && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
    })
}

fn download(
    release: &Release,
    installed: bool,
    cancelled: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Operation cancelled".into());
    }
    let asset = release
        .asset(installed)
        .ok_or("No compatible Windows x64 package in this release")?;
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .ok_or("Release has no SHA256SUMS.txt")?;
    if !trusted_asset(&asset.browser_download_url)
        || !trusted_asset(&sums.browser_download_url)
        || asset.size == 0
        || asset.size > MAX_DOWNLOAD
    {
        return Err("Invalid release asset".into());
    }
    let client = client()?;
    let mut sums_bytes = Vec::new();
    client
        .get(&sums.browser_download_url)
        .send()?
        .error_for_status()?
        .take(65537)
        .read_to_end(&mut sums_bytes)?;
    if sums_bytes.len() > 65536 {
        return Err("Checksum list exceeds size limit".into());
    }
    let expected = checksum(std::str::from_utf8(&sums_bytes)?, &asset.name)
        .ok_or("Missing or invalid SHA-256 checksum")?;
    let folder = crate::model::directory()
        .join("updates")
        .join(crate::model::now().to_string());
    fs::create_dir_all(&folder)?;
    let path = folder.join(&asset.name);
    let partial = path.with_extension("partial");
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut response = client
            .get(&asset.browser_download_url)
            .send()?
            .error_for_status()?;
        let mut file = fs::File::create(&partial)?;
        let mut hash = Sha256::new();
        let mut done = 0;
        let mut buffer = [0; 65536];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Operation cancelled".into());
            }
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > asset.size {
                return Err("Download exceeds declared size".into());
            }
            file.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            progress(done, asset.size);
        }
        if done != asset.size || format!("{:x}", hash.finalize()) != expected {
            return Err("Download size or SHA-256 mismatch".into());
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&partial, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result?;
    Ok((path, expected))
}

fn installed_product() -> Option<String> {
    use windows_sys::{Win32::System::ApplicationInstallationAndServicing::*, core::w};
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let mut product = [0; 39];
    for index in 0..100 {
        if unsafe {
            MsiEnumRelatedProductsW(
                w!("{C46BEBD7-5641-4709-A6E8-20433C31DA2C}"),
                0,
                index,
                product.as_mut_ptr(),
            )
        } != 0
        {
            break;
        }
        let mut path = [0; 32768];
        let mut size = path.len() as u32;
        let state = unsafe {
            MsiGetComponentPathW(
                product.as_ptr(),
                w!("{18D3CD4F-814C-4D73-9DBE-F8EFD854919F}"),
                path.as_mut_ptr(),
                &mut size,
            )
        };
        if state == INSTALLSTATE_LOCAL
            && PathBuf::from(String::from_utf16_lossy(&path[..size as usize]))
                .canonicalize()
                .ok()
                .as_ref()
                == Some(&executable)
        {
            return Some(String::from_utf16_lossy(&product[..38]));
        }
    }
    None
}

#[derive(Serialize)]
struct InstallPlan<'a> {
    executable: PathBuf,
    package: &'a Path,
    sha256: &'a str,
    data: PathBuf,
    application: u32,
    supervisor: Option<u32>,
    product: Option<&'a str>,
    downgrade: bool,
    recovery: Option<&'a Recovery>,
}

pub fn install(downloaded: &Downloaded, product: Option<&str>) -> Result<(), String> {
    let Downloaded {
        release,
        package,
        sha256,
        recovery,
    } = downloaded;
    let folder = package
        .parent()
        .ok_or("Invalid update path")?
        .join(format!("install-{}", crate::model::now()));
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let plan = folder.join("install.json");
    let script = folder.join("install.ps1");
    crate::model::write_json(
        &plan,
        &InstallPlan {
            executable: std::env::current_exe().map_err(|e| e.to_string())?,
            package,
            sha256,
            data: crate::model::directory(),
            application: std::process::id(),
            supervisor: std::env::var("RUSTCLIP_SUPERVISOR_PID")
                .ok()
                .and_then(|s| s.parse().ok()),
            product,
            recovery: recovery.as_ref(),
            downgrade: release
                .version()
                .zip(Version::parse(VERSION).ok())
                .is_some_and(|(a, b)| a < b),
        },
    )?;
    fs::write(&script, include_str!("updates/install.ps1")).map_err(|e| e.to_string())?;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg(plan)
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    while !folder.join("ready").exists() {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("Update helper did not start; application remains open".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    fs::write(folder.join("go"), b"install").map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Downloads a published portable EXE from GitHub"]
    fn github_portable_download_has_valid_checksum() {
        let cancelled = AtomicBool::new(false);
        let releases = list_releases(&cancelled).unwrap();
        let release = releases
            .iter()
            .find(|r| !r.prerelease && r.asset(false).is_some())
            .unwrap();
        let (package, expected) = download(release, false, &cancelled, |_, _| {}).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&package).unwrap())),
            expected
        );
        fs::remove_file(package).unwrap();
    }

    #[test]
    fn cancelled_requests_do_not_start_network_operations() {
        assert_eq!(
            list_releases(&AtomicBool::new(true))
                .unwrap_err()
                .to_string(),
            "Operation cancelled"
        );
    }

    #[test]
    fn update_notice_requires_a_newer_allowed_release() {
        let mut updates = Updates::default();
        assert!(updates.available(false).is_none());
        updates.releases = serde_json::from_value(serde_json::json!([
            {"tag_name":"v999.0.0-rc.1", "prerelease":true, "draft":false, "assets":[]},
            {"tag_name":"v998.0.0", "prerelease":false, "draft":false, "assets":[]},
            {"tag_name":"v0.0.0", "prerelease":false, "draft":false, "assets":[]}
        ]))
        .unwrap();
        assert_eq!(updates.available(false).unwrap().tag_name, "v998.0.0");
        assert_eq!(updates.available(true).unwrap().tag_name, "v999.0.0-rc.1");
        updates.releases.drain(..2);
        assert!(updates.available(true).is_none());
    }

    #[test]
    fn checksums_require_exact_filename_and_full_hash() {
        let hash = "ab".repeat(32);
        assert_eq!(
            checksum(&format!("{hash}  rustclip.exe\n"), "rustclip.exe"),
            Some(hash.clone())
        );
        assert_eq!(
            checksum(&format!("{hash} *rustclip.exe\r\n"), "rustclip.exe"),
            Some(hash)
        );
        assert_eq!(checksum("abc  rustclip.exe", "rustclip.exe"), None);
        assert_eq!(
            checksum(&format!("{}  other.exe", "00".repeat(32)), "rustclip.exe"),
            None
        );
    }

    #[test]
    fn asset_urls_stay_in_project_releases() {
        assert!(trusted_asset(
            "https://github.com/slavonkanarov/rustclip/releases/download/v1.0.0/rustclip.exe"
        ));
        assert!(!trusted_asset(
            "https://github.com/slavonkanarov/other/releases/download/v1/x"
        ));
        assert!(!trusted_asset(
            "http://github.com/slavonkanarov/rustclip/releases/download/v1/x"
        ));
    }

    #[test]
    fn versions_use_semantic_order_and_correct_package() {
        let mut r: Release = serde_json::from_str(r#"{"tag_name":"v0.10.0","body":null,"prerelease":false,"draft":false,"assets":[{"name":"rustclip.exe","browser_download_url":"","size":1},{"name":"RustClip-0.10.0-x64.msi","browser_download_url":"","size":1}]}"#).unwrap();
        assert!(r.version().unwrap() > Version::parse("0.9.0").unwrap());
        assert_eq!(r.asset(true).unwrap().name, "RustClip-0.10.0-x64.msi");
        assert_eq!(r.asset(false).unwrap().name, "rustclip.exe");
        r.tag_name = "v0.10.0-rc.1".into();
        assert!(r.version().unwrap() < Version::parse("0.10.0").unwrap());
    }
}
