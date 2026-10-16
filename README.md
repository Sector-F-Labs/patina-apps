# patina-apps

Folder apps, as on a Mac. An app is a folder, `Name.app`, with an `app.toml` saying how to run
it; copy one into `~/Apps` to install it, delete it to uninstall. The format and its rules are
Patina OS's ADR-0004 (format 1); why apps are folders is ADR-0002 and ADR-0005.

```toml
format = 1
name = "Calendar"
exec = "bin/calendar"          # or [exec] with linux-x86_64 = "...", freebsd-x86_64 = "..."
id = "com.sectorflabs.Calendar"
summary = "Calendar"
icon = "icon.svg"
```

This crate is the one place those rules live, for everything that shows or runs folder apps:

- **patina-desktop's launcher** lists `~/Apps` beside the `.desktop` entries.
- **Files** shows a `.app` anywhere as one item with its icon, and runs it when it's opened.

```rust
for (folder, app) in patina_apps::list(&patina_apps::dir().unwrap()) {
    match app {
        Ok(app) => app.launch()?,
        Err(why) => eprintln!("{}: {why}", folder.display()),
    }
}
```

The `icon` feature adds `icon::render`, which draws an SVG or PNG icon to RGBA at a size.

## Use

It isn't on crates.io; depend on it by git:

```toml
patina-apps = { git = "ssh://divan@remotes.divanv.com/~/Remotes/src/com/sectorflabs/patina-apps", features = ["icon"] }
```

with `net.git-fetch-with-cli = true` in `.cargo/config.toml`, so cargo uses your ssh setup.

## Develop

`make check`: tests and clippy.
