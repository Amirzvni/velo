use crate::error::{Result, VeloError};
use crate::fsutil::sanitize_file_name;
use crate::types::{DownloadSpec, ProbeResult};
use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH,
    CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::{Client, StatusCode};
use url::Url;

/// Headers we never let the caller override, and schemes we allow.
fn is_forbidden_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "connection" | "transfer-encoding" | "range"
    )
}

pub fn build_headers(spec: &DownloadSpec) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in &spec.headers {
        if is_forbidden_header(k) {
            continue;
        }
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| VeloError::Other(format!("bad header name: {k}")))?;
        let value =
            HeaderValue::from_str(v).map_err(|_| VeloError::Other(format!("bad header: {k}")))?;
        map.insert(name, value);
    }
    Ok(map)
}

/// Only http/https. Blocks file://, ftp://, data:, etc.
pub fn validate_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|e| VeloError::InvalidUrl(e.to_string()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        s => Err(VeloError::InvalidUrl(format!("scheme not allowed: {s}"))),
    }
}

/// Ask for the first byte only. Works on servers that reject HEAD, and the
/// 206 + Content-Range answer proves range support in one round trip.
pub async fn probe(client: &Client, spec: &DownloadSpec) -> Result<ProbeResult> {
    let url = validate_url(&spec.url)?;
    let mut headers = build_headers(spec)?;
    headers.insert(RANGE, HeaderValue::from_static("bytes=0-0"));

    let resp = client.get(url.clone()).headers(headers).send().await?;
    let status = resp.status();
    if !status.is_success() && status != StatusCode::PARTIAL_CONTENT {
        return Err(VeloError::BadStatus(status.as_u16()));
    }

    let h = resp.headers();
    let supports_range = status == StatusCode::PARTIAL_CONTENT
        || h.get(ACCEPT_RANGES).and_then(|v| v.to_str().ok()) == Some("bytes");

    let total_size = if status == StatusCode::PARTIAL_CONTENT {
        h.get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next().map(|s| s.to_string()))
            .and_then(|s| s.parse::<u64>().ok())
    } else {
        h.get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok())
    };

    let etag = h.get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
    let last_modified = h.get(LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(str::to_string);
    let mime = h
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let final_url = resp.url().clone();
    let disposition = h.get(CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()).map(str::to_string);

    let file_name = match &spec.file_name {
        Some(n) => sanitize_file_name(n)?,
        None => detect_file_name(disposition.as_deref(), &final_url, mime.as_deref())?,
    };

    Ok(ProbeResult {
        final_url: final_url.to_string(),
        file_name,
        total_size,
        supports_range,
        etag,
        last_modified,
        mime,
    })
}

/// Content-Disposition wins, then the URL path, then a generic fallback.
pub fn detect_file_name(
    disposition: Option<&str>,
    url: &Url,
    mime: Option<&str>,
) -> Result<String> {
    if let Some(d) = disposition {
        if let Some(name) = parse_disposition(d) {
            if let Ok(safe) = sanitize_file_name(&name) {
                return Ok(safe);
            }
        }
    }

    if let Some(seg) = url.path_segments().and_then(|s| s.filter(|p| !p.is_empty()).next_back()) {
        let decoded = percent_decode_str(seg).decode_utf8_lossy().to_string();
        if let Ok(safe) = sanitize_file_name(&decoded) {
            return Ok(safe);
        }
    }

    let ext = match mime {
        Some("application/pdf") => ".pdf",
        Some("application/zip") => ".zip",
        Some("video/mp4") => ".mp4",
        Some("audio/mpeg") => ".mp3",
        _ => ".bin",
    };
    Ok(format!("download{ext}"))
}

/// Handles both `filename="x"` and RFC 5987 `filename*=UTF-8''x`.
fn parse_disposition(value: &str) -> Option<String> {
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename*=") {
            let encoded = rest.rsplit('\'').next()?;
            return Some(percent_decode_str(encoded).decode_utf8_lossy().to_string());
        }
    }
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_forms() {
        assert_eq!(parse_disposition("attachment; filename=\"a b.zip\"").unwrap(), "a b.zip");
        assert_eq!(
            parse_disposition("attachment; filename*=UTF-8''%D9%81%D8%A7%DB%8C%D9%84.pdf").unwrap(),
            "فایل.pdf"
        );
    }

    #[test]
    fn falls_back_to_url_path() {
        let u = Url::parse("https://x.com/files/setup%20v2.exe?token=1").unwrap();
        assert_eq!(detect_file_name(None, &u, None).unwrap(), "setup v2.exe");
    }

    #[test]
    fn rejects_non_http() {
        assert!(validate_url("file:///c:/windows").is_err());
    }
}
