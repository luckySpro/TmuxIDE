use regex::Regex;

/// 在终端输出写入 Git 之前过滤常见凭据。只是降低风险，不能保证识别所有敏感信息。
pub struct Redactor {
    rules: Vec<(Regex, &'static str)>,
}

impl Default for Redactor {
    fn default() -> Self {
        let rules = [
            (
                r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
                "[已过滤：私钥]",
            ),
            (
                r"\b(ghp|gho|ghu|ghs|ghr|github_pat)_[A-Za-z0-9_]{20,}",
                "[已过滤：GitHub Token]",
            ),
            (r"\bglpat-[A-Za-z0-9_\-]{20,}", "[已过滤：GitLab Token]"),
            (r"\bAKIA[0-9A-Z]{16}\b", "[已过滤：AWS Key]"),
            (r"\bsk-[A-Za-z0-9_\-]{20,}", "[已过滤：API Key]"),
            (r"\bxox[abpors]-[A-Za-z0-9\-]{10,}", "[已过滤：Slack Token]"),
            (r"(?i)\b(bearer)\s+[A-Za-z0-9._\-]{16,}", "$1 [已过滤]"),
            (
                r#"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key)(\s*[=:]\s*)("[^"]*"|'[^']*'|\S+)"#,
                "$1$2[已过滤]",
            ),
        ];
        Redactor {
            rules: rules
                .into_iter()
                .map(|(re, rep)| (Regex::new(re).expect("内置规则有效"), rep))
                .collect(),
        }
    }
}

impl Redactor {
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (re, rep) in &self.rules {
            out = re.replace_all(&out, *rep).into_owned();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secrets() {
        let r = Redactor::default();
        let text = "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789\npassword: hunter2\nAKIAABCDEFGHIJKLMNOP\nls -la";
        let out = r.redact(text);
        assert!(!out.contains("ghp_abc"));
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("AKIAABCDEFGHIJKLMNOP"));
        assert!(out.contains("ls -la"));
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        assert_eq!(r.redact(key), "[已过滤：私钥]");
    }
}
