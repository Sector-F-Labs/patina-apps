//! Drawing an icon file (SVG or PNG) to a square of RGBA pixels, for apps
//! that show app icons, and the icon theme to find them in ([`theme`]). It
//! reads and decodes files, so call it off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Tried in turn when the chosen theme isn't installed: Yaru, which Ubuntu
/// and FreeBSD both package, then GTK's own.
const FALLBACKS: [&str; 2] = ["Yaru", "Adwaita"];

/// How long a read of the setting stands, so a long-running program (the
/// bar) follows a change without asking for every icon.
const STALE: Duration = Duration::from_secs(5);

/// The icon theme to look icons up in: the desktop's choice (Patina's
/// Settings writes GNOME's `icon-theme` for now) where it's installed, else
/// Yaru, else Adwaita, else hicolor, which lookups fall back to anyway. A
/// choice left behind by a removed desktop (breeze-dark, once KDE went)
/// would otherwise leave only hicolor, with no folder or file-type icons.
pub fn theme() -> String {
    static LAST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, theme)) = last.as_ref()
        && at.elapsed() < STALE
    {
        return theme.clone();
    }
    let theme = choose(chosen().as_deref(), &theme_dirs());
    *last = Some((Instant::now(), theme.clone()));
    theme
}

/// The desktop's choice of icon theme, if it has one.
fn chosen() -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let value = String::from_utf8_lossy(&out.stdout);
    Some(value.trim().trim_matches('\'').to_string()).filter(|v| !v.is_empty())
}

/// `chosen` if it's installed in `dirs`, else the first fallback that is.
fn choose(chosen: Option<&str>, dirs: &[PathBuf]) -> String {
    chosen
        .into_iter()
        .chain(FALLBACKS)
        .find(|t| installed(t, dirs))
        .unwrap_or("hicolor")
        .to_string()
}

/// Where icon themes are, as the icon theme spec looks: `~/.icons`, then
/// `icons` in each XDG data directory.
fn theme_dirs() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = env("HOME").map(PathBuf::from);
    let data_home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = env("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.map(|h| h.join(".icons"))
        .into_iter()
        .chain(data_home.map(|d| d.join("icons")))
        .chain(std::env::split_paths(&data_dirs).map(|d| d.join("icons")))
        .collect()
}

/// Whether theme `name` is in one of `dirs`: its folder, with an index.theme.
fn installed(name: &str, dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| d.join(name).join("index.theme").is_file())
}

/// The icon at `path` as `size` x `size` premultiplied RGBA, keeping its
/// aspect ratio, centred; `None` if it isn't an SVG or PNG that decodes.
pub fn render(path: &Path, size: u32) -> Option<Vec<u8>> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "svg" | "svgz" => svg(&std::fs::read(path).ok()?, size),
        "png" => png(path, size),
        _ => None,
    }
}

/// SVG data as `size` x `size` premultiplied RGBA, as tiny-skia renders.
pub fn svg(data: &[u8], size: u32) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let scale = size as f32 / w.max(h);
    let dx = (size as f32 - w * scale) / 2.0;
    let dy = (size as f32 - h * scale) / 2.0;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.take())
}

/// Premultiplied RGBA, resized to fit and centred.
fn png(path: &Path, size: u32) -> Option<Vec<u8>> {
    let image = image::open(path).ok()?.into_rgba8();
    let (w, h) = image.dimensions();
    let scale = size as f32 / w.max(h) as f32;
    let (fw, fh) = (
        ((w as f32 * scale).round() as u32).clamp(1, size),
        ((h as f32 * scale).round() as u32).clamp(1, size),
    );
    let fitted = image::imageops::resize(&image, fw, fh, image::imageops::FilterType::Triangle);
    let mut square = image::RgbaImage::new(size, size);
    image::imageops::overlay(
        &mut square,
        &fitted,
        ((size - fw) / 2) as i64,
        ((size - fh) / 2) as i64,
    );
    Some(
        square
            .pixels()
            .flat_map(|p| {
                let [r, g, b, a] = p.0;
                let m = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
                [m(r), m(g), m(b), a]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chosen_theme_if_installed_else_yaru_else_adwaita() {
        let dir = std::env::temp_dir().join(format!("patina-apps-themes-{}", std::process::id()));
        let add = |name: &str| {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join("index.theme"), "[Icon Theme]\n").unwrap();
        };
        add("Adwaita");
        let dirs = [dir.clone()];
        assert_eq!(choose(Some("breeze-dark"), &dirs), "Adwaita");
        add("Yaru");
        add("Papirus");
        assert_eq!(choose(Some("breeze-dark"), &dirs), "Yaru");
        assert_eq!(choose(None, &dirs), "Yaru");
        assert_eq!(choose(Some("Papirus"), &dirs), "Papirus");
        assert_eq!(choose(Some("Papirus"), &[]), "hicolor");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn draws_svg_and_png_to_the_size_asked() {
        let dir = std::env::temp_dir().join(format!("patina-apps-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("wide.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
        )
        .unwrap();
        let rgba = render(&svg, 32).expect("svg draws");
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Wide, so centred with transparent bands above and below.
        assert_eq!(rgba[3], 0);
        let middle = (16 * 32 + 16) * 4;
        assert_eq!(&rgba[middle..middle + 4], [255, 0, 0, 255]);

        let png = dir.join("dot.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 128, 128]))
            .save(&png)
            .unwrap();
        let rgba = render(&png, 8).expect("png draws");
        assert_eq!(rgba.len(), 8 * 8 * 4);
        assert_eq!(rgba[3], 128);
        assert_eq!(rgba[2], 64, "premultiplied");

        let tall = dir.join("tall.png");
        image::RgbaImage::from_pixel(2, 4, image::Rgba([255, 255, 255, 255]))
            .save(&tall)
            .unwrap();
        let rgba = render(&tall, 8).unwrap();
        assert_eq!(rgba[3], 0, "centred, not stretched");

        assert!(render(&dir.join("x.xpm"), 8).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
