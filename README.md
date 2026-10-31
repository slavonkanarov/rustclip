# RustClip

**English** | [Русский](docs/README.ru.md)

RustClip is a clipboard history manager for Windows, written in Rust. It saves copied text, images, and supported clipboard formats so you can find an earlier clip and paste it into the active application. It stays in the system tray: closing the window hides it, and a global shortcut opens the history again.

The history includes search by content, source application, or label; favorites; previews; plain-text paste; clip export; and a history backup. You can configure global shortcuts and exclude applications from capture. A separate **Secrets** tab stores entries in an encrypted vault with folders and application bindings. Regular clipboard history is not encrypted.

RustClip targets Windows x64. Building from source requires the Rust MSVC toolchain, Visual Studio C++ Build Tools, and the Windows SDK.

## Download

Get the MSI installer or portable ZIP from [GitHub Releases](https://github.com/slavonkanarov/rustclip/releases). The MSI installs for all users and requires administrator rights; the ZIP runs without installation. Installation does not launch RustClip or enable startup with Windows.

## Run from source

```powershell
cargo run --release
```

The executable is `target/release/rustclip.exe`. To inspect the interface without clipboard monitoring or global shortcuts, run:

```powershell
target/release/rustclip.exe --demo
```

RustClip reports shortcuts that conflict with other applications. Closing the window hides it in the tray. To quit, use **File → Exit** or right-click the tray icon. Preferences (`F4`) and the clip editor (`F2`) open in separate windows.

To start RustClip when you sign in, enable **Settings → General → Start when Windows starts** and click **Save**. This setting is off by default, applies to the current user, and needs no administrator rights. Disable it before moving or deleting the EXE; after moving the EXE, enable it again from its new location.

## Windows packages and releases

The [Windows MSI workflow](.github/workflows/windows-msi.yml) runs on pushes to `dev`, `release`, and `main`; on `v*` tags; on pull requests to those branches; and manually from **Actions → Windows MSI → Run workflow**. It checks formatting and tests, builds a Windows x64 EXE and MSI, then tests installation, upgrades, and removal. Builds use MSVC with a statically linked CRT.

Successful workflow runs provide the `rustclip-windows-x64` Actions artifact for 90 days:

- `RustClip-<version>-x64.msi`: installer with a license screen, installation to `C:\Program Files\RustClip`, and a Start menu shortcut; requires administrator rights.
- `RustClip-<version>-windows-x64.zip`: EXE, license, and English and Russian READMEs for portable use.
- `rustclip.exe`, `LICENSE`, and `SHA256SUMS.txt`: individual executable, license, and checksums.

### Versioning

`Cargo.toml` sets the minimum `major.minor.patch` version, for example `0.1.0`. The next patch number is calculated from published stable tags in the same series: `v0.1.0`, then `v0.1.1`, and so on. This calculation does not change `Cargo.toml`.

| Branch or tag | Build version | Distribution |
| --- | --- | --- |
| `dev` | `<next version>-dev.<Actions run number>` | Actions artifact |
| `release` (optional) | `<next version>-rc.<Actions run number>` | Actions artifact; an RC tag creates a prerelease |
| `main` | `<next version>` | Actions artifact; a successful build automatically creates a stable tag and final release |

Changes normally go to `dev`, then to `main` through a pull request. Use the optional `release` branch to check a release candidate before merging into `main`. Each successful `main` build tags its commit as `v<version>` and publishes a final release. Stable tags must point to commits on `main`; RC tags must point to commits on `release`. To start a new major or minor series, change `Cargo.toml`; patch versions advance automatically. The build version appears in the **Help** menu and package names. Windows Installer uses the numeric version without a suffix, so the next stable release upgrades the previous one.

Quit RustClip through the tray before upgrading. An MSI can upgrade an existing installation, including a build with the same version; it cannot install an older version over a newer one. The MSI and portable EXE both store user data in `%APPDATA%\RustClip`, outside the installation directory.

When uninstalling the MSI interactively, you can **keep data** (the default) or select **Also delete this user's RustClip data**. The latter permanently removes the current user's history, settings, encrypted secrets, backups, and exports from `%APPDATA%\RustClip`. It does not remove other users' data or directories selected with `RUSTCLIP_DATA_DIR`. Upgrades preserve data.

Silent uninstallation keeps data unless you set `DELETEUSERDATA=1`:

```powershell
msiexec /x RustClip-0.1.0-x64.msi /qn
# Also remove the current user's data:
msiexec /x RustClip-0.1.0-x64.msi /qn DELETEUSERDATA=1
```

### Local installation without MSI

From the repository root, run:

```powershell
.\scripts\install-local.ps1
```

The script requests administrator rights, builds a release EXE, installs it with the icon, license, and both READMEs to `C:\Program Files\RustClip`, and creates a Start menu shortcut. If RustClip was installed through MSI, the script removes the old MSI registration before copying files. It updates an existing per-user shortcut and startup entry, and removes an old local copy in `%LOCALAPPDATA%\Programs\RustClip` after installation. WiX and the .NET SDK are not needed. The script does not start RustClip; history, settings, and secrets in `%APPDATA%\RustClip` are kept. Quit RustClip through the tray and rerun the script to update it.

The first Cargo build may download dependencies. If the tools and dependencies are already available locally, use:

```powershell
.\scripts\install-local.ps1 -Offline
```

This script does not create an entry in Windows' installed-apps list. To remove this installation, quit RustClip, delete `C:\Program Files\RustClip` and `%ProgramData%\Microsoft\Windows\Start Menu\Programs\RustClip.lnk`, and remove any remaining per-user shortcut. You can keep or remove `%APPDATA%\RustClip` separately.

### Build an MSI locally

You need Rust MSVC, Visual Studio C++ Build Tools with the Windows SDK, and .NET SDK 8. Run from the repository root:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
cargo +stable-x86_64-pc-windows-msvc build --release --locked --target x86_64-pc-windows-msvc
dotnet tool install wix --version 6.0.2 --tool-path ./target/tools/wix --add-source https://api.nuget.org/v3/index.json
./target/tools/wix/wix.exe extension add -g WixToolset.UI.wixext/6.0.2
./target/tools/wix/wix.exe extension add -g WixToolset.Util.wixext/6.0.2
./scripts/package-windows.ps1 -Wix ./target/tools/wix/wix.exe
```

Packages are written to `target/package`. Without `RUSTCLIP_BUILD_VERSION`, a local build uses the base version from `Cargo.toml`. The MSI version must fit `major.minor.patch` with a maximum of `255.255.65535`. The installer is not digitally signed.

## Use RustClip

Configure global shortcuts in **Settings → Hotkeys**: click **…** to record a combination such as `Ctrl+Shift+Up`, then save the shortcut and settings. Ctrl, Shift, Alt, Win, letters, digits, arrows, F1–F24, and common control keys are supported. Duplicate and invalid combinations are rejected; RustClip reports combinations already registered by another application.

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+D` | Open clipboard history |
| `Ctrl+Shift+↑` / `↓` | Select an older/newer clip and show a central preview; holding the keys starts cycling after a pause, every 120 ms; optionally paste on release |
| `Ctrl+Shift+V` | Paste the current clipboard contents as plain text |
| `Enter` / `Shift+Enter` | Paste the selected clip / paste as plain text, unless focus is in a text field |
| `F2` / `F4` | Open the clip editor / preferences |
| `Esc` | Hide the window and central preview |
| `Alt+X` | Quit while the main window is open |

### Secrets

Open **Settings → Secrets** to create a PIN of at least four digits or a password. Until then, the Secrets tab shows a warning and does not save secrets. Names, values, and application bindings are stored separately from clipboard history in `secrets.vault`, encrypted with ChaCha20-Poly1305; a key is derived from the PIN or password with Argon2. The unlocked session and decrypted entries remain in memory until **Lock vault** or exit; returning to history hides them from view. Only encrypted vault data is written to disk, including temporary files used while saving.

Double-click a secret to paste it into the active window, or click its copy icon (**Copy value**) to leave it in the clipboard. The secret temporarily passes through the system clipboard, but these actions do not add it to RustClip's history. After unlocking the vault, `Ctrl+Shift+D` can open secrets filtered for the active application when a matching secret exists. Older templates are moved into the encrypted vault when it is opened.

Every secret belongs to a folder. Names can repeat across folders; legacy secrets without folders are moved to **Imported**. You can search for secrets associated with an application across folders. Secret values are hidden by default and can be revealed per entry. A folder's application binding applies to its secrets unless a secret has its own binding. Secret and folder editors open separately. **Choose application…** lists the five most recent active applications and allows manual entry.

## Data

By default, RustClip uses `%APPDATA%\RustClip`: `clips/`, `settings.json`, `Backup/`, `exports/`, and `secrets.vault`. Set `RUSTCLIP_DATA_DIR` for an isolated test directory. History, settings, exports, and backups are unencrypted; the contents of `secrets.vault` are encrypted.

History entries are stored as separate JSON files. This initial format loads history into memory and is not designed for multi-gigabyte histories.

## Checks

```powershell
cargo fmt --check
cargo test --offline
cargo check --offline
```

Licensed under [GPL-3.0-or-later](LICENSE).
