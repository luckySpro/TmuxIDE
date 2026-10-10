//! 当设置了 `TMUXIDE_TMUX_BIN`（由 vendor/tmux 下的脚本静态编译出的 tmux）时，把它嵌入最终二进制。
//! 未设置时（本地开发）退回使用系统 PATH 中的 tmux。

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(bundled_tmux)");
    println!("cargo::rerun-if-env-changed=TMUXIDE_TMUX_BIN");
    println!("cargo::rerun-if-env-changed=TMUXIDE_TMUX_VERSION");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("tmux.bin");
    match env::var("TMUXIDE_TMUX_BIN") {
        Ok(src) if !src.is_empty() => {
            println!("cargo::rerun-if-changed={src}");
            fs::copy(&src, &out).unwrap_or_else(|e| panic!("无法读取 TMUXIDE_TMUX_BIN={src}: {e}"));
            println!("cargo::rustc-cfg=bundled_tmux");
            let version = env::var("TMUXIDE_TMUX_VERSION").unwrap_or_else(|_| "bundled".into());
            println!("cargo::rustc-env=TMUXIDE_BUNDLED_TMUX_VERSION={version}");
        }
        _ => {
            // 占位文件，保证 include_bytes! 路径存在
            fs::write(&out, b"").unwrap();
        }
    }
}
