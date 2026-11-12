use url::Url;

/// The decoded path of `url` with repeated and trailing separators removed, except for
/// the root path.
pub fn path_without_trailing_slash(url: &Url) -> String {
    let decoded = percent_decode(url.path());
    let joined = decoded
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if decoded.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(url: &str) -> String {
        path_without_trailing_slash(&Url::parse(url).unwrap())
    }

    #[test]
    fn strips_a_trailing_slash() {
        assert_eq!(path("file:///tmp/example/"), "/tmp/example");
    }

    #[test]
    fn strips_multiple_trailing_slashes() {
        assert_eq!(path("file:///tmp/example///"), "/tmp/example");
    }

    #[test]
    fn decodes_the_path() {
        assert_eq!(path("file:///tmp/example%20directory/"), "/tmp/example directory");
    }

    #[test]
    fn preserves_the_root() {
        assert_eq!(path("file:///"), "/");
    }
}
