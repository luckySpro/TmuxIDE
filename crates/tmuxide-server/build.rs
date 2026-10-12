//! 设置了 `TMUXIDE_CLOUDFLARED_BIN`（CI 下载并校验过的 cloudflared）时，把它嵌入最终二进制，
//! 远程访问的 Cloudflare 隧道因此不需要用户另外安装。未设置时（本地开发）退回 PATH 中的 cloudflared。

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(bundled_cloudflared)");
    println!("cargo::rerun-if-env-changed=TMUXIDE_CLOUDFLARED_BIN");
    println!("cargo::rerun-if-env-changed=TMUXIDE_CLOUDFLARED_VERSION");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("cloudflared.bin");
    match env::var("TMUXIDE_CLOUDFLARED_BIN") {
        Ok(src) if !src.is_empty() => {
            println!("cargo::rerun-if-changed={src}");
            let data = fs::read(&src).unwrap_or_else(|e| panic!("无法读取 TMUXIDE_CLOUDFLARED_BIN={src}: {e}"));
            fs::write(&out, &data).unwrap();
            println!("cargo::rustc-cfg=bundled_cloudflared");
            // 构建时算好指纹，运行时不用每次扫描几十 MB
            let mut h: u64 = 0xcbf29ce484222325;
            for b in &data {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            println!("cargo::rustc-env=TMUXIDE_CLOUDFLARED_HASH={h:016x}");
            let version = env::var("TMUXIDE_CLOUDFLARED_VERSION").unwrap_or_else(|_| "bundled".into());
            println!("cargo::rustc-env=TMUXIDE_BUNDLED_CLOUDFLARED_VERSION={version}");
        }
        _ => fs::write(&out, b"").unwrap(),
    }
}
