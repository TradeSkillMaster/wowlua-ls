//! Zed extension for the WoW Lua language server.
//!
//! Zed extensions run as WebAssembly and cannot host the analysis engine, so all
//! this crate does is resolve a `wowlua_ls` executable for the current platform
//! and hand the command back to Zed, which then speaks LSP to it over stdio.

use std::fs;

use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

/// Repository that publishes the `wowlua_ls` release binaries.
const RELEASE_REPO: &str = "TradeSkillMaster/wowlua-ls";

/// Name of the server executable, both on `$PATH` and within a release asset.
const BINARY_NAME: &str = "wowlua_ls";

struct ServerBinary {
    path: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

struct WowLuaExtension {
    /// Path of the binary this extension last downloaded. Kept so that repeated
    /// server starts within a session don't re-query the GitHub releases API.
    downloaded_binary_path: Option<String>,
}

impl WowLuaExtension {
    fn server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<ServerBinary> {
        let configured = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let args = configured
            .as_ref()
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_default();
        let env = configured
            .as_ref()
            .and_then(|binary| binary.env.clone())
            .map(|env| env.into_iter().collect())
            .unwrap_or_default();

        // An explicit `binary.path` wins, then a `wowlua_ls` on the user's
        // `$PATH` (typically a `cargo build --release` checkout), and only then a
        // binary downloaded from the latest GitHub release.
        let path = match configured.and_then(|binary| binary.path) {
            Some(path) => path,
            None => match worktree.which(BINARY_NAME) {
                Some(path) => path,
                None => self.download_binary(language_server_id)?,
            },
        };

        Ok(ServerBinary { path, args, env })
    }

    /// Downloads the release binary for this platform into the extension's work
    /// directory, returning a path relative to it. Release binaries embed the WoW
    /// API stubs, so the single file is all that needs to be fetched.
    fn download_binary(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        if let Some(path) = &self.downloaded_binary_path
            && fs::metadata(path).is_ok_and(|stat| stat.is_file())
        {
            return Ok(path.clone());
        }

        let (os, arch) = zed::current_platform();
        // Resolve the asset name before the network call: no release will ever
        // carry a binary for an unsupported platform, and a failed release fetch
        // would otherwise mask that with a GitHub error the user can't act on.
        let asset_name = release_asset_name(os, arch)?;

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = match zed::latest_github_release(
            RELEASE_REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        ) {
            Ok(release) => release,
            // Offline, or the GitHub API is rate-limiting us: a binary from an
            // earlier run beats leaving the project without a language server.
            Err(err) => {
                let previous = previously_downloaded_binary(os).ok_or(err)?;
                self.downloaded_binary_path = Some(previous.clone());
                return Ok(previous);
            }
        };

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!("wowlua_ls {} has no release asset named {asset_name:?}", release.version)
            })?;

        let version_dir = format!("wowlua_ls-{}", release.version);
        let binary_path = format!("{version_dir}/{BINARY_NAME}{}", executable_suffix(os));

        if !fs::metadata(&binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            if let Err(err) = install_binary(asset, &version_dir, &binary_path) {
                // Leave nothing behind: a partial file under `binary_path` would
                // read as a finished install on the next start, and a stale
                // version directory would confuse the offline fallback.
                fs::remove_dir_all(&version_dir).ok();
                return Err(err);
            }
        }

        // The work directory holds nothing but our downloads, so anything outside
        // the current version directory is a superseded binary. This runs on every
        // resolve rather than only after a download, so a run interrupted between
        // installing and cleaning up can't leave a second version directory for
        // the offline fallback to choose from.
        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                if entry.file_name().to_str() != Some(&version_dir) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.downloaded_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

/// Puts the release asset in place under `binary_path`, and only once it is
/// whole: release assets are bare executables, and `download_file` writes
/// straight to the path it is given, so an interrupted download — or a failed
/// `make_file_executable` — under the final name would look like a finished
/// install to every later start, with nothing left to retry it. Staging the file
/// and renaming it last means `binary_path` never exists half-written.
fn install_binary(
    asset: &zed::GithubReleaseAsset,
    version_dir: &str,
    binary_path: &str,
) -> Result<()> {
    // Zed writes the download path without creating parent directories.
    fs::create_dir_all(version_dir)
        .map_err(|err| format!("failed to create {version_dir}: {err}"))?;

    let staged = format!("{binary_path}.part");
    zed::download_file(
        &asset.download_url,
        &staged,
        zed::DownloadedFileType::Uncompressed,
    )
    .map_err(|err| format!("failed to download {}: {err}", asset.name))?;
    zed::make_file_executable(&staged)?;
    fs::rename(&staged, binary_path).map_err(|err| format!("failed to install {binary_path}: {err}"))
}

/// A binary left in the work directory by an earlier download. Installs are
/// atomic, so any hit is a complete binary, and each one deletes the versions
/// before it, so it is the newest.
fn previously_downloaded_binary(os: zed::Os) -> Option<String> {
    let binary = format!("{BINARY_NAME}{}", executable_suffix(os));
    fs::read_dir(".").ok()?.flatten().find_map(|entry| {
        let candidate = format!("{}/{binary}", entry.file_name().to_str()?);
        fs::metadata(&candidate)
            .is_ok_and(|stat| stat.is_file())
            .then_some(candidate)
    })
}

/// Name of the release asset built for the given platform.
fn release_asset_name(os: zed::Os, arch: zed::Architecture) -> Result<String> {
    let target = match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
        (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
        (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-gnu",
        // Windows on ARM runs x64 executables under emulation, and there is no
        // aarch64 Windows release build to prefer over it.
        (zed::Os::Windows, zed::Architecture::X8664 | zed::Architecture::Aarch64) => {
            "x86_64-pc-windows-msvc"
        }
        (os, arch) => {
            return Err(format!(
                "wowlua_ls publishes no release binary for {os:?} {arch:?}. Build it from source \
                 (https://github.com/TradeSkillMaster/wowlua-ls) and point \
                 `lsp.wowlua-ls.binary.path` at it in your Zed settings.",
            ));
        }
    };
    Ok(format!("{BINARY_NAME}-{target}{}", executable_suffix(os)))
}

fn executable_suffix(os: zed::Os) -> &'static str {
    match os {
        zed::Os::Mac | zed::Os::Linux => "",
        zed::Os::Windows => ".exe",
    }
}

impl zed::Extension for WowLuaExtension {
    fn new() -> Self {
        Self {
            downloaded_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = self.server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            // `wowlua_ls` with no subcommand starts the language server on stdio.
            command: binary.path,
            args: binary.args,
            env: binary.env,
        })
    }
}

zed::register_extension!(WowLuaExtension);
