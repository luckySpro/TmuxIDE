use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

static TMUX_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tmux.bin"));

pub fn has_bundled() -> bool {
    !TMUX_BIN.is_empty()
}

pub fn bundled_version() -> Option<&'static str> {
    #[cfg(bundled_tmux)]
    {
        Some(env!("TMUXIDE_BUNDLED_TMUX_VERSION"))
    }
    #[cfg(not(bundled_tmux))]
    {
        None
    }
}

/// FNV-1a，用于给释放出的文件命名，版本变化时自动换新文件
fn fingerprint(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 把内置 tmux 释放到 `dir`，已存在且大小一致则直接复用
pub fn extract(dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = dir.join(format!("tmux-{:016x}", fingerprint(TMUX_BIN)));
    if fs::metadata(&target)
        .map(|m| m.len() == TMUX_BIN.len() as u64)
        .unwrap_or(false)
    {
        return Ok(target);
    }
    let tmp = dir.join(format!(".tmux-{}.tmp", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(TMUX_BIN)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&tmp, &target)?;
    // 清理旧版本
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p != target && name.starts_with("tmux-") {
                let _ = fs::remove_file(p);
            }
        }
    }
    Ok(target)
}
