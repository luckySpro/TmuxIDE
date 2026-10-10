use std::path::Path;

/// 把以家目录开头的路径写成 `~/...`，便于跨机器恢复。
pub fn collapse_home(path: &str, home: Option<&Path>) -> String {
    if let Some(home) = home.and_then(|h| h.to_str()) {
        let home = home.trim_end_matches('/');
        if !home.is_empty() {
            if path == home {
                return "~".into();
            }
            if let Some(rest) = path.strip_prefix(home) {
                if rest.starts_with('/') {
                    return format!("~{rest}");
                }
            }
        }
    }
    path.to_string()
}

/// `collapse_home` 的反操作。
pub fn expand_home(path: &str, home: Option<&Path>) -> String {
    if let Some(home) = home.and_then(|h| h.to_str()) {
        if path == "~" {
            return home.to_string();
        }
        if let Some(rest) = path.strip_prefix("~/") {
            return format!("{}/{}", home.trim_end_matches('/'), rest);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_roundtrip() {
        let home = Path::new("/Users/lucky");
        assert_eq!(collapse_home("/Users/lucky/code", Some(home)), "~/code");
        assert_eq!(collapse_home("/Users/lucky", Some(home)), "~");
        assert_eq!(collapse_home("/Users/luckyx/a", Some(home)), "/Users/luckyx/a");
        assert_eq!(expand_home("~/code", Some(Path::new("/home/l"))), "/home/l/code");
        assert_eq!(expand_home("/tmp", Some(home)), "/tmp");
    }
}
