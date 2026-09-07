use crate::error::{Result, VeloError};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Characters Windows forbids in a file name, plus control chars.
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Reserved DOS device names. `CON.txt` is still reserved on Windows.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Turn anything a server sends us into a name that cannot escape out_dir.
pub fn sanitize_file_name(raw: &str) -> Result<String> {
    // Only ever keep the last path component; kills ../../ and C:\ prefixes.
    let base = raw
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('.')
        .to_string();

    let cleaned: String = base
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(&c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();

    let cleaned = cleaned.trim_end_matches([' ', '.']).to_string();

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return Err(VeloError::UnsafeFileName(raw.to_string()));
    }

    let stem = cleaned.split('.').next().unwrap_or("").to_uppercase();
    let cleaned = if RESERVED.contains(&stem.as_str()) {
        format!("_{cleaned}")
    } else {
        cleaned
    };

    // Windows MAX_PATH friendly.
    let cleaned = if cleaned.len() > 200 {
        cleaned[..200].to_string()
    } else {
        cleaned
    };

    Ok(cleaned)
}

/// If `name` exists, return `name (1)`, `name (2)`, ...
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..10_000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem} ({}){ext}", std::process::id()))
}

/// Create the target file and reserve its size up front so the OS can lay it
/// out contiguously and every segment can write straight to its own offset.
pub fn create_preallocated(path: &Path, size: Option<u64>) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    if let Some(size) = size {
        file.set_len(size)?;
    }
    Ok(file)
}

/// Positional write that does not move the shared file cursor, so many threads
/// can write different offsets of the same file at the same time.
#[cfg(windows)]
pub fn write_at(file: &File, offset: u64, buf: &[u8]) -> Result<()> {
    use std::os::windows::fs::FileExt;
    let mut written = 0usize;
    while written < buf.len() {
        let n = file.seek_write(&buf[written..], offset + written as u64)?;
        if n == 0 {
            return Err(VeloError::Other("short write".into()));
        }
        written += n;
    }
    Ok(())
}

#[cfg(unix)]
pub fn write_at(file: &File, offset: u64, buf: &[u8]) -> Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_traversal() {
        assert_eq!(sanitize_file_name("../../evil.exe").unwrap(), "evil.exe");
        assert_eq!(
            sanitize_file_name("C:\\Windows\\sys.dll").unwrap(),
            "sys.dll"
        );
        assert_eq!(sanitize_file_name("a<b>c.txt").unwrap(), "a_b_c.txt");
        assert_eq!(sanitize_file_name("CON.txt").unwrap(), "_CON.txt");
        assert!(sanitize_file_name("   ").is_err());
    }
}
