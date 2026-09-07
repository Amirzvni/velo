//! Where Velo puts things, per platform.
//!
//! Windows: C:\Users\<you>\Downloads\Velo  +  %APPDATA%\Velo
//! macOS:   ~/Downloads/Velo              +  ~/Library/Application Support/Velo
//! Linux:   ~/Downloads/Velo              +  ~/.local/share/Velo

use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "Velo";

/// The OS Downloads folder, with a Velo subfolder so we never mix our files
/// into whatever else lives there. Falls back to ./downloads if the OS has no
/// Downloads dir (rare, e.g. a stripped container).
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .map(|d| d.join(APP_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from("./downloads"))
}

/// Where the database, logs and config live.
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir().map(|d| d.join(APP_DIR_NAME)).unwrap_or_else(|| PathBuf::from("./.velo"))
}

pub fn database_path() -> PathBuf {
    app_data_dir().join("velo.db")
}

/// Create both directories if they are missing.
pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(default_download_dir())?;
    std::fs::create_dir_all(app_data_dir())?;
    Ok(())
}

/// Sort finished files into subfolders by kind. Beginners like this, and it is
/// off by default for anyone who does not.
pub fn category_for(mime: Option<&str>, file_name: &str) -> &'static str {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" => "Video",
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" => "Audio",
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "bz2" => "Archives",
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => "Programs",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" => "Documents",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" => "Images",
        _ => match mime.map(|m| m.split('/').next().unwrap_or("")) {
            Some("video") => "Video",
            Some("audio") => "Audio",
            Some("image") => "Images",
            _ => "Other",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_by_extension_then_mime() {
        assert_eq!(category_for(None, "movie.MKV"), "Video");
        assert_eq!(category_for(None, "setup.exe"), "Programs");
        assert_eq!(category_for(Some("audio/mpeg"), "track"), "Audio");
        assert_eq!(category_for(None, "weird.xyz"), "Other");
    }

    #[test]
    fn download_dir_is_velo_folder_or_fallback() {
        let d = default_download_dir();
        // On a real desktop this is <Downloads>/Velo. On a bare environment
        // with no Downloads dir (CI, WSL) we fall back to ./downloads.
        assert!(d.ends_with(APP_DIR_NAME) || d.ends_with("downloads"));
    }
}
