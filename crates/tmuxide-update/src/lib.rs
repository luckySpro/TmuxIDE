//! 自动升级：签名的升级目录、下载加速与官方回退、命令行版自更新。
//!
//! 发布流程（CI）为每个正式版本生成 `update.json`，并用发布私钥签名为 `update.json.sig`（Tauri / minisign 格式）。
//! 程序内只包含公钥：目录签名不对、版本无法解析时一律视为“没有可信的新版本”，不会降级、不会强制升级。
//!
//! - 目录给出最新版本、最低支持版本（低于它必须升级）、更新说明，以及命令行版各平台压缩包的地址和 SHA-256。
//! - 桌面版安装包由 Tauri 升级插件下载，并用同一把公钥校验安装包自己的签名。
//! - 先走 GH-Proxy 加速，失败再回到 github.com 官方地址；加速只是传输，所有内容仍以签名为准。

use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const REPO: &str = "luckySpro/TmuxIDE";
pub const MIRROR: &str = "https://gh-proxy.com/";
const MAX_CATALOG: u64 = 256 * 1024;
const MAX_PACKAGE: u64 = 300 * 1024 * 1024;

/// 发布公钥（`tauri signer generate` 生成的 .pub 内容，base64）。为空表示此版本不支持自动升级。
pub fn pubkey() -> &'static str {
    match option_env!("TMUXIDE_UPDATER_PUBKEY") {
        Some(k) if !k.trim().is_empty() => k.trim(),
        _ => include_str!("../updater.pub").trim(),
    }
}

pub fn enabled() -> bool {
    !pubkey().is_empty()
}

/// 已签名的升级目录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub version: String,
    /// 低于这个版本的客户端必须升级
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: String,
    /// 命令行版：平台（如 `linux-x86_64`、`macos-arm64`）→ 压缩包
    #[serde(default)]
    pub cli: std::collections::BTreeMap<String, Package>,
    /// 桌面版：Tauri 平台键（如 `darwin-aarch64-app`）→ 安装包签名。
    /// Tauri 的 latest.json 本身没有签名，桌面版安装前核对它给出的签名与这里一致，防止被换成旧版本的安装包。
    #[serde(default)]
    pub desktop: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Package {
    /// github.com 官方下载地址
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// 下载源：先加速再官方。测试时可用 `TMUXIDE_UPDATE_BASE` 指向本地目录服务（内容仍须通过签名校验）。
pub fn sources(official: &str) -> Vec<String> {
    if let Ok(base) = std::env::var("TMUXIDE_UPDATE_BASE") {
        let name = official.rsplit('/').next().unwrap_or_default();
        return vec![format!("{}/{name}", base.trim_end_matches('/'))];
    }
    vec![format!("{MIRROR}{official}"), official.to_string()]
}

pub fn latest_url(file: &str) -> String {
    format!("https://github.com/{REPO}/releases/latest/download/{file}")
}

pub fn release_page(version: &str) -> String {
    format!(
        "https://github.com/{REPO}/releases/tag/v{}",
        version.trim_start_matches('v')
    )
}

fn agent(platform_roots: bool) -> ureq::Agent {
    let roots = if platform_roots {
        ureq::tls::RootCerts::PlatformVerifier
    } else {
        ureq::tls::RootCerts::WebPki
    };
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent(format!("TmuxIDE/{}", env!("CARGO_PKG_VERSION")))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(roots).build())
        .build()
        .into()
}

/// 先用内置根证书；证书不被信任时（例如公司网络的 HTTPS 代理）改用系统信任的证书再试一次
fn call(url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    match agent(false).get(url).call() {
        Ok(r) => Ok(r),
        Err(e) if e.to_string().contains("certificate") => {
            tracing::info!("{url}：内置根证书不信任对方证书（{e}），改用系统证书");
            agent(true).get(url).call().with_context(|| format!("下载 {url} 失败"))
        }
        Err(e) => Err(e).with_context(|| format!("下载 {url} 失败")),
    }
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>> {
    let resp = call(url)?;
    let mut buf = Vec::new();
    resp.into_body()
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut buf)
        .with_context(|| format!("读取 {url} 失败"))?;
    if buf.len() as u64 > limit {
        bail!("{url} 超过大小上限");
    }
    Ok(buf)
}

/// 用发布公钥校验 Tauri 格式的签名（.sig 文件内容为 base64 编码的 minisign 签名）
pub fn verify(data: &[u8], sig_b64: &str, pubkey_b64: &str) -> Result<()> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let pk_text = String::from_utf8(b64.decode(pubkey_b64.trim()).context("公钥格式不正确")?)?;
    let sig_text = String::from_utf8(b64.decode(sig_b64.trim()).context("签名格式不正确")?)?;
    let pk = minisign_verify::PublicKey::decode(&pk_text).map_err(|e| anyhow!("公钥无效：{e}"))?;
    let sig = minisign_verify::Signature::decode(&sig_text).map_err(|e| anyhow!("签名无效：{e}"))?;
    pk.verify(data, &sig, false).map_err(|_| anyhow!("签名校验失败"))
}

/// 解析并校验目录
pub fn parse_catalog(data: &[u8], sig: &str, pubkey: &str) -> Result<Catalog> {
    verify(data, sig, pubkey).context("升级目录")?;
    let c: Catalog = serde_json::from_slice(data).context("升级目录格式不正确")?;
    semver::Version::parse(&c.version).context("升级目录中的版本号不正确")?;
    if let Some(m) = &c.min_version {
        semver::Version::parse(m).context("升级目录中的最低版本不正确")?;
    }
    Ok(c)
}

/// 下载并校验最新的升级目录：依次尝试加速与官方地址，返回第一个签名有效的目录
pub fn fetch_catalog() -> Result<Catalog> {
    fetch_catalog_raw().map(|(c, _, _)| c)
}

/// 同 [`fetch_catalog`]，另外返回目录原文和签名（用于离线缓存，读取时重新校验）
pub fn fetch_catalog_raw() -> Result<(Catalog, String, String)> {
    if !enabled() {
        bail!("此版本没有配置升级签名公钥");
    }
    let mut last = anyhow!("没有可用的下载地址");
    for (data_url, sig_url) in sources(&latest_url("update.json"))
        .into_iter()
        .zip(sources(&latest_url("update.json.sig")))
    {
        let r = get(&data_url, MAX_CATALOG).and_then(|d| {
            let sig = String::from_utf8(get(&sig_url, 4096)?)?;
            let c = parse_catalog(&d, &sig, pubkey())?;
            Ok((c, String::from_utf8(d)?, sig))
        });
        match r {
            Ok(c) => return Ok(c),
            Err(e) => {
                tracing::debug!("升级目录 {data_url}：{e:#}");
                last = e;
            }
        }
    }
    Err(last)
}

/// 比较版本：`a` 比 `b` 新
pub fn newer(a: &str, b: &str) -> bool {
    match (
        semver::Version::parse(a.trim_start_matches('v')),
        semver::Version::parse(b.trim_start_matches('v')),
    ) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

/// 当前版本是否低于目录要求的最低版本
pub fn required(c: &Catalog, current: &str) -> bool {
    c.min_version.as_deref().is_some_and(|m| newer(m, current)) && newer(&c.version, current)
}

/// 命令行版当前平台的名称，与发布文件名一致
pub fn cli_platform() -> Option<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        _ => return None,
    };
    let arch = match (os, std::env::consts::ARCH) {
        ("macos", "aarch64") => "arm64",
        (_, "x86_64") => "x86_64",
        ("linux", "aarch64") => "aarch64",
        _ => return None,
    };
    Some(format!("{os}-{arch}"))
}

/// 下载命令行版压缩包（加速 → 官方），校验 SHA-256 后返回字节
pub fn download_package(p: &Package, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>> {
    if p.size > MAX_PACKAGE {
        bail!("安装包过大");
    }
    let mut last = anyhow!("没有可用的下载地址");
    for url in sources(&p.url) {
        let r = (|| -> Result<Vec<u8>> {
            let resp = call(&url)?;
            // 只读到目录给出的大小多 1 字节：更大的内容直接判为不一致
            let mut reader = resp.into_body().into_reader().take(p.size + 1);
            let mut buf = Vec::with_capacity(p.size as usize);
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() as u64 > p.size {
                    bail!("校验失败：{url} 比签名目录给出的大小更大");
                }
                progress(buf.len() as u64, p.size);
            }
            let got = hex(&Sha256::digest(&buf));
            if !got.eq_ignore_ascii_case(&p.sha256) {
                bail!("校验失败：{url} 的 SHA-256 与签名目录不一致");
            }
            Ok(buf)
        })();
        match r {
            Ok(b) => return Ok(b),
            Err(e) => {
                tracing::warn!("{e:#}");
                last = e;
            }
        }
    }
    Err(last)
}

/// 从压缩包中取出 `tmuxide`，原子替换 `target`（先写同目录临时文件再改名）
pub fn install_cli(tarball: &[u8], target: &Path) -> Result<()> {
    let dir = target.parent().ok_or_else(|| anyhow!("无法确定安装目录"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    let mut binary = None;
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        if entry.header().entry_type().is_file()
            && path.file_name().is_some_and(|n| n == "tmuxide")
            && path.components().count() == 2
        {
            let mut buf = Vec::new();
            entry.take(MAX_PACKAGE).read_to_end(&mut buf)?;
            binary = Some(buf);
            break;
        }
    }
    let binary = binary.ok_or_else(|| anyhow!("压缩包里没有 tmuxide"))?;
    let tmp: PathBuf = dir.join(format!(".tmuxide-update-{}", std::process::id()));
    std::fs::write(&tmp, &binary).with_context(|| format!("无法写入 {}（可能需要管理员权限）", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("无法替换 {}", target.display()));
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 计算文件内容的 SHA-256（发布脚本与测试使用）
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 由 `tauri signer generate` 生成的测试密钥及其对 b"hello" 的签名（仅用于测试）
    const TEST_PUB: &str = include_str!("../tests/test.key.pub");
    const TEST_SIG: &str = include_str!("../tests/hello.sig");

    #[test]
    fn verifies_tauri_signatures() {
        verify(b"hello", TEST_SIG, TEST_PUB).unwrap();
        assert!(verify(b"hellO", TEST_SIG, TEST_PUB).is_err());
    }

    #[test]
    fn compares_versions() {
        assert!(newer("0.3.0", "0.2.9"));
        assert!(newer("v0.10.0", "0.9.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("garbage", "0.1.0"));
        let mut c = Catalog {
            version: "0.3.0".into(),
            min_version: Some("0.2.5".into()),
            notes: String::new(),
            pub_date: String::new(),
            cli: Default::default(),
            desktop: Default::default(),
        };
        assert!(required(&c, "0.2.0"));
        assert!(!required(&c, "0.2.5"));
        c.min_version = None;
        assert!(!required(&c, "0.1.0"));
    }

    #[test]
    fn installs_cli_from_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let mut tgz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tgz, flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            let mut h = tar::Header::new_gnu();
            h.set_size(4);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "tmuxide-9.9.9-linux-x86_64/tmuxide", &b"new!"[..])
                .unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let target = dir.path().join("tmuxide");
        std::fs::write(&target, b"old").unwrap();
        install_cli(&tgz, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new!");
    }
}
