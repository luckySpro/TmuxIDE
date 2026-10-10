/// tmux 会话名不能包含 `.` 和 `:`（目标语法分隔符）；同时作为文件名使用，所以限制为安全字符。
pub fn valid_session_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
}

/// 把任意字符串转换为可用作会话名/目录名的形式。
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(64).collect::<String>();
    if out.is_empty() {
        "unnamed".into()
    } else {
        out
    }
}

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh",
];

/// 判断 tmux `pane_current_command` 是否只是交互 shell（恢复时无需重新执行）。
pub fn is_shell(cmd: &str) -> bool {
    let base = cmd.rsplit('/').next().unwrap_or(cmd).trim_start_matches('-');
    SHELLS.contains(&base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_session_name("dev"));
        assert!(valid_session_name("开发-1"));
        assert!(!valid_session_name("a.b"));
        assert!(!valid_session_name("a:b"));
        assert!(!valid_session_name("../x"));
        assert!(!valid_session_name(""));
        assert_eq!(sanitize_name(" my host.local "), "my-host-local");
        assert_eq!(sanitize_name("///"), "unnamed");
    }

    #[test]
    fn shells() {
        assert!(is_shell("zsh"));
        assert!(is_shell("-bash"));
        assert!(is_shell("/bin/sh"));
        assert!(!is_shell("nvim"));
    }
}
