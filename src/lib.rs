//! Folder apps, as on a Mac: an app is a folder, `Name.app`, with an
//! `app.toml` saying how to run it (Patina OS ADR-0002, and ADR-0004 for
//! format 1). Copy one into `~/Apps` to install it; delete it to uninstall.
//!
//! This crate is what everything that shows or runs them shares: the Patina
//! desktop's launcher, which lists `~/Apps`, and Files, which opens a
//! `.app` wherever it is. With the `icon` feature, [`icon::render`] draws an
//! app's icon.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

#[cfg(feature = "icon")]
pub mod icon;

/// The app.toml formats this crate understands.
pub const FORMAT: u32 = 1;

/// A folder app that can run on this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The `.app` folder.
    pub folder: PathBuf,
    /// Reverse-DNS, or the folder's name without `.app`.
    pub id: String,
    pub name: String,
    /// One line under the name, like a .desktop file's GenericName.
    pub summary: Option<String>,
    pub comment: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    /// An SVG or PNG inside the folder, if it names one that's there.
    pub icon: Option<PathBuf>,
    /// The program for this platform, inside the folder and executable.
    pub program: PathBuf,
    /// Passed to the program, after it.
    pub args: Vec<String>,
    /// Run it in a terminal.
    pub terminal: bool,
}

/// app.toml as written; keys this crate doesn't know are ignored, so
/// optional ones can be added without a new format.
#[derive(Deserialize)]
struct Manifest {
    format: u32,
    name: String,
    exec: Exec,
    id: Option<String>,
    summary: Option<String>,
    comment: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    icon: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    terminal: bool,
}

/// One program, or one for each platform (`linux-x86_64`, `freebsd-x86_64`).
#[derive(Deserialize)]
#[serde(untagged)]
enum Exec {
    One(String),
    PerPlatform(BTreeMap<String, String>),
}

/// This platform, as `[exec]` names them: `{os}-{arch}`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `~/Apps`, where installed apps live.
pub fn dir() -> Option<PathBuf> {
    Some(home()?.join("Apps"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A folder named `*.app`, not hidden.
pub fn is_app_folder(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    !name.starts_with('.') && name.ends_with(".app") && name.len() > 4 && path.is_dir()
}

/// Every app folder in `dir`, sorted, each read for this platform or with
/// the reason it can't run.
pub fn list(dir: &Path) -> Vec<(PathBuf, Result<App, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_app_folder(p))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .map(|folder| {
            let app = read(&folder);
            (folder, app)
        })
        .collect()
}

/// The app in `folder`, for this platform.
pub fn read(folder: &Path) -> Result<App, String> {
    read_for(folder, &platform())
}

/// The app in `folder`, for `platform` (`{os}-{arch}`). It's refused, with
/// the reason, if format 1's rules say it can't be run correctly.
pub fn read_for(folder: &Path, platform: &str) -> Result<App, String> {
    let manifest = manifest(folder)?;
    let program = match &manifest.exec {
        Exec::One(path) => path,
        Exec::PerPlatform(paths) => paths
            .get(platform)
            .ok_or_else(|| format!("no program for {platform}"))?,
    };
    let program = inside(folder, program).ok_or("exec must be a path inside the folder")?;
    if !executable(&program) {
        return Err(format!("{} isn't an executable file", program.display()));
    }
    Ok(App {
        folder: folder.to_path_buf(),
        id: manifest.id.unwrap_or_else(|| stem(folder).to_string()),
        name: manifest.name,
        summary: manifest.summary,
        comment: manifest.comment,
        keywords: manifest.keywords,
        categories: manifest.categories,
        icon: icon_in(folder, manifest.icon.as_deref()),
        program,
        args: manifest.args,
        terminal: manifest.terminal,
    })
}

/// The icon of the app in `folder`, even one that can't run here, so a
/// folder of apps for another platform still looks like apps.
pub fn icon(folder: &Path) -> Option<PathBuf> {
    icon_in(folder, manifest(folder).ok()?.icon.as_deref())
}

/// The folder's name without `.app`.
pub fn stem(folder: &Path) -> &str {
    let name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.strip_suffix(".app").unwrap_or(name)
}

fn manifest(folder: &Path) -> Result<Manifest, String> {
    let text =
        std::fs::read_to_string(folder.join("app.toml")).map_err(|e| format!("app.toml: {e}"))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| format!("app.toml: {e}"))?;
    if manifest.format != FORMAT {
        return Err(format!(
            "app.toml is format {}; this reads format {FORMAT}",
            manifest.format
        ));
    }
    Ok(manifest)
}

/// A missing or unsafe icon only loses the icon.
fn icon_in(folder: &Path, icon: Option<&str>) -> Option<PathBuf> {
    icon.and_then(|i| inside(folder, i)).filter(|i| i.is_file())
}

/// `relative` within `folder`, or `None` if it's absolute or climbs out.
fn inside(folder: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    let plain = path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    (plain && !relative.is_empty()).then(|| folder.join(path))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

impl App {
    /// The command line: the program and its args, inside a terminal if
    /// the app asks for one.
    pub fn command_line(&self) -> Vec<String> {
        let mut line = vec![self.program.to_string_lossy().into_owned()];
        line.extend(self.args.iter().cloned());
        if self.terminal {
            line.splice(0..0, [terminal(), "-e".into()]);
        }
        line
    }

    /// Starts the app from the user's home folder, in a process group of
    /// its own so it outlives whatever started it.
    pub fn launch(&self) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        let line = self.command_line();
        let mut command = Command::new(&line[0]);
        command
            .args(&line[1..])
            .stdin(Stdio::null())
            .process_group(0);
        if let Some(home) = home() {
            command.current_dir(home);
        }
        let mut child = command.spawn()?;
        // Reaped on a thread, so a long-running caller collects no zombies.
        std::thread::spawn(move || child.wait());
        Ok(())
    }
}

/// The terminal for apps that ask for one: `$TERMINAL`, foot, or Debian's
/// x-terminal-emulator. All of them take `-e command...`.
pub fn terminal() -> String {
    if let Some(t) = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty()) {
        return t;
    }
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(name).exists()))
    };
    ["foot", "x-terminal-emulator"]
        .into_iter()
        .find(|t| on_path(t))
        .unwrap_or("foot")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of apps for one test, removed when it's dropped.
    struct Apps(PathBuf);

    impl Apps {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("patina-apps-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Apps(dir)
        }

        /// `folder` with `toml` as its app.toml and executables at `programs`.
        fn add(&self, folder: &str, toml: &str, programs: &[&str]) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let folder = self.0.join(folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("app.toml"), toml).unwrap();
            for p in programs {
                let path = folder.join(p);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "#!/bin/sh\n").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            folder
        }
    }

    impl Drop for Apps {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_an_app_folder() {
        let apps = Apps::new("reads");
        let folder = apps.add(
            "Teleprinter.app",
            "format = 1\nname = \"Teleprinter\"\nexec = \"bin/teleprinter\"\n\
             summary = \"Terminal\"\nkeywords = [\"shell\"]\nargs = [\"--window\"]\n\
             categories = [\"System\"]\nicon = \"icon.png\"\nbase = \"patina-os:1@sha256:00\"\n\
             later = \"ignored\"\n",
            &["bin/teleprinter"],
        );
        std::fs::write(folder.join("icon.png"), b"").unwrap();
        let app = read_for(&folder, "linux-x86_64").unwrap();
        assert_eq!(app.name, "Teleprinter");
        assert_eq!(app.id, "Teleprinter");
        assert_eq!(app.summary.as_deref(), Some("Terminal"));
        assert_eq!(app.keywords, ["shell"]);
        assert_eq!(app.categories, ["System"]);
        assert_eq!(app.program, folder.join("bin/teleprinter"));
        let program = folder
            .join("bin/teleprinter")
            .to_string_lossy()
            .into_owned();
        assert_eq!(app.command_line(), [program, "--window".to_string()]);
        assert_eq!(app.icon, Some(folder.join("icon.png")));
        assert_eq!(icon(&folder), Some(folder.join("icon.png")));
        assert_eq!(stem(&folder), "Teleprinter");
    }

    #[test]
    fn picks_the_program_for_this_platform() {
        let apps = Apps::new("platform");
        let folder = apps.add(
            "Tool.app",
            "format = 1\nname = \"Tool\"\n[exec]\nlinux-x86_64 = \"bin/linux/tool\"\n\
             freebsd-x86_64 = \"bin/freebsd/tool\"\n",
            &["bin/linux/tool", "bin/freebsd/tool"],
        );
        let on = |p| read_for(&folder, p).map(|a| a.program);
        assert!(on("linux-x86_64").unwrap().ends_with("bin/linux/tool"));
        assert!(on("freebsd-x86_64").unwrap().ends_with("bin/freebsd/tool"));
        assert_eq!(
            on("linux-aarch64").unwrap_err(),
            "no program for linux-aarch64"
        );
    }

    #[test]
    fn refuses_what_it_cant_run_correctly() {
        let apps = Apps::new("refuses");
        let err = |folder: &str, toml: &str, programs: &[&str]| {
            read_for(&apps.add(folder, toml, programs), "linux-x86_64").unwrap_err()
        };
        let out = err(
            "Out.app",
            "format = 1\nname = \"O\"\nexec = \"../x\"\n",
            &[],
        );
        assert!(out.contains("inside"));
        let abs = err(
            "Abs.app",
            "format = 1\nname = \"A\"\nexec = \"/bin/sh\"\n",
            &[],
        );
        assert!(abs.contains("inside"));
        let gone = err(
            "Gone.app",
            "format = 1\nname = \"G\"\nexec = \"bin/g\"\n",
            &[],
        );
        assert!(gone.contains("executable"));
        let new = err(
            "New.app",
            "format = 2\nname = \"N\"\nexec = \"n\"\n",
            &["n"],
        );
        assert!(new.contains("format 2"));
        assert!(err("Bad.app", "name = \"B\"\n", &[]).starts_with("app.toml"));
    }

    #[test]
    fn an_unsafe_icon_is_only_dropped() {
        let apps = Apps::new("icon");
        let folder = apps.add(
            "I.app",
            "format = 1\nname = \"I\"\nexec = \"i\"\nicon = \"../../etc/x.png\"\n",
            &["i"],
        );
        assert_eq!(read_for(&folder, "linux-x86_64").unwrap().icon, None);
        assert_eq!(icon(&folder), None);
    }

    #[test]
    fn an_app_for_another_platform_keeps_its_icon() {
        let apps = Apps::new("elsewhere");
        let folder = apps.add(
            "Far.app",
            "format = 1\nname = \"F\"\nicon = \"i.svg\"\n[exec]\nplan9-mips = \"f\"\n",
            &[],
        );
        std::fs::write(folder.join("i.svg"), b"").unwrap();
        assert!(read_for(&folder, "linux-x86_64").is_err());
        assert_eq!(icon(&folder), Some(folder.join("i.svg")));
    }

    #[test]
    fn a_terminal_app_runs_in_one() {
        let apps = Apps::new("terminal");
        let folder = apps.add(
            "T.app",
            "format = 1\nname = \"T\"\nexec = \"t\"\nterminal = true\n",
            &["t"],
        );
        let line = read_for(&folder, "linux-x86_64").unwrap().command_line();
        assert_eq!(line[1], "-e");
        assert!(line[2].ends_with("/t"));
    }

    #[test]
    fn lists_only_app_folders() {
        let apps = Apps::new("lists");
        apps.add("B.app", "format = 1\nname = \"B\"\nexec = \"b\"\n", &["b"]);
        apps.add("A.app", "format = 1\nname = \"A\"\nexec = \"a\"\n", &["a"]);
        apps.add("notes", "format = 1\nname = \"N\"\nexec = \"n\"\n", &["n"]);
        apps.add(
            ".Hidden.app",
            "format = 1\nname = \"H\"\nexec = \"h\"\n",
            &["h"],
        );
        apps.add("Broken.app", "format = 1\n", &[]);
        let listed = list(&apps.0);
        let names: Vec<&str> = listed.iter().map(|(f, _)| stem(f)).collect();
        assert_eq!(names, ["A", "B", "Broken"]);
        let runnable: Vec<String> = listed
            .into_iter()
            .filter_map(|(_, a)| a.ok())
            .map(|a| a.name)
            .collect();
        assert_eq!(runnable, ["A", "B"]);
        assert!(list(&apps.0.join("nowhere")).is_empty());
    }
}
