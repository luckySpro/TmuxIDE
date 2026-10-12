//! 远程访问的持久化状态：已配对设备（只保存凭证的 SHA-256）、PIN（Argon2）和访问记录。
//!
//! 文件位于 `<数据目录>/remote/`，权限 0600。

use std::{
    collections::VecDeque,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::write_atomic;

/// 连续输错 PIN 多少次后锁定设备
pub const MAX_PIN_FAILURES: u32 = 5;
const AUDIT_KEEP: usize = 300;
const AUDIT_FILE_MAX: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// 设备凭证的 SHA-256（十六进制），凭证本身只在手机上
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub last_ip: String,
    #[serde(default)]
    pub read_only: bool,
    /// PIN 输错次数过多被锁定，需要在电脑上解除
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub pin_failures: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    pin_hash: Option<String>,
    #[serde(default)]
    devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    /// pair | pair_fail | unlock | unlock_fail | locked | attach | detach | revoke | expire | start | stop | error
    pub kind: String,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub ip: String,
    pub detail: String,
}

pub struct Store {
    state_file: PathBuf,
    audit_file: PathBuf,
    state: Mutex<State>,
    audit: Mutex<VecDeque<AuditEvent>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("PIN 错误，还可以再试 {0} 次")]
    Wrong(u32),
    #[error("PIN 连续输错次数过多，这台设备已被锁定，请在电脑上解除")]
    Locked,
    #[error("还没有设置 PIN")]
    NotSet,
}

impl Store {
    pub fn open(data_dir: &Path) -> anyhow::Result<Store> {
        let dir = data_dir.join("remote");
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let state_file = dir.join("devices.json");
        let state: State = match std::fs::read_to_string(&state_file) {
            Ok(t) => serde_json::from_str(&t)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e.into()),
        };
        let audit_file = dir.join("audit.jsonl");
        let audit = load_audit(&audit_file);
        Ok(Store {
            state_file,
            audit_file,
            state: Mutex::new(state),
            audit: Mutex::new(audit),
        })
    }

    fn persist(&self, state: &State) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
        write_private(&self.state_file, text.as_bytes())
    }

    // ---------- PIN ----------

    pub fn has_pin(&self) -> bool {
        self.state.lock().unwrap().pin_hash.is_some()
    }

    /// 设置 PIN；空字符串表示清除
    pub fn set_pin(&self, pin: &str) -> anyhow::Result<()> {
        let hash = if pin.is_empty() {
            None
        } else {
            if pin.len() < 6 || pin.len() > 32 || !pin.chars().all(|c| c.is_ascii_digit()) {
                anyhow::bail!("PIN 需要 6 到 32 位数字");
            }
            let salt_bytes: [u8; 16] = rand::random();
            let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
            Some(
                Argon2::default()
                    .hash_password(pin.as_bytes(), &salt)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .to_string(),
            )
        };
        let mut st = self.state.lock().unwrap();
        st.pin_hash = hash;
        self.persist(&st)?;
        Ok(())
    }

    /// 校验 PIN，并记录该设备的失败次数
    pub fn check_pin(&self, device_id: &str, pin: &str) -> Result<(), PinError> {
        let mut st = self.state.lock().unwrap();
        let Some(hash) = st.pin_hash.clone() else {
            return Err(PinError::NotSet);
        };
        let Some(dev) = st.devices.iter_mut().find(|d| d.id == device_id) else {
            return Err(PinError::Locked);
        };
        if dev.locked {
            return Err(PinError::Locked);
        }
        let ok = PasswordHash::new(&hash)
            .map(|h| Argon2::default().verify_password(pin.as_bytes(), &h).is_ok())
            .unwrap_or(false);
        let result = if ok {
            dev.pin_failures = 0;
            Ok(())
        } else {
            dev.pin_failures += 1;
            if dev.pin_failures >= MAX_PIN_FAILURES {
                dev.locked = true;
                Err(PinError::Locked)
            } else {
                Err(PinError::Wrong(MAX_PIN_FAILURES - dev.pin_failures))
            }
        };
        let _ = self.persist(&st);
        result
    }

    // ---------- 设备 ----------

    pub fn devices(&self) -> Vec<Device> {
        self.state.lock().unwrap().devices.clone()
    }

    pub fn device(&self, id: &str) -> Option<Device> {
        self.state.lock().unwrap().devices.iter().find(|d| d.id == id).cloned()
    }

    /// 新增设备，返回设备记录和只交给手机的明文凭证
    pub fn add_device(&self, name: &str, ip: &str) -> anyhow::Result<(Device, String)> {
        let token = random_hex(32);
        let now = Utc::now();
        let dev = Device {
            id: random_hex(8),
            name: clean_name(name),
            token_hash: sha256_hex(&token),
            created_at: now,
            last_seen: now,
            last_ip: ip.to_string(),
            read_only: false,
            locked: false,
            pin_failures: 0,
        };
        let mut st = self.state.lock().unwrap();
        st.devices.push(dev.clone());
        self.persist(&st)?;
        Ok((dev, token))
    }

    /// 按明文凭证查找设备；过期设备会被删除并返回 Err(过期设备名)
    pub fn find_by_token(&self, token: &str, max_idle_days: u32) -> Result<Option<Device>, String> {
        if token.len() != 64 {
            return Ok(None);
        }
        let hash = sha256_hex(token);
        let mut st = self.state.lock().unwrap();
        let Some(pos) = st.devices.iter().position(|d| d.token_hash == hash) else {
            return Ok(None);
        };
        let dev = st.devices[pos].clone();
        if Utc::now() - dev.last_seen > chrono::Duration::days(max_idle_days as i64) {
            st.devices.remove(pos);
            let _ = self.persist(&st);
            return Err(dev.name);
        }
        Ok(Some(dev))
    }

    /// 更新最近使用时间（每分钟最多写一次文件）
    pub fn touch(&self, id: &str, ip: &str) {
        let mut st = self.state.lock().unwrap();
        let Some(dev) = st.devices.iter_mut().find(|d| d.id == id) else {
            return;
        };
        let now = Utc::now();
        let changed_ip = dev.last_ip != ip;
        if changed_ip || now - dev.last_seen > chrono::Duration::minutes(1) {
            dev.last_seen = now;
            dev.last_ip = ip.to_string();
            let _ = self.persist(&st);
        }
    }

    pub fn update_device(
        &self,
        id: &str,
        name: Option<&str>,
        read_only: Option<bool>,
        unlock: bool,
    ) -> anyhow::Result<Device> {
        let mut st = self.state.lock().unwrap();
        let dev = st
            .devices
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or_else(|| anyhow::anyhow!("设备不存在"))?;
        if let Some(n) = name {
            dev.name = clean_name(n);
        }
        if let Some(r) = read_only {
            dev.read_only = r;
        }
        if unlock {
            dev.locked = false;
            dev.pin_failures = 0;
        }
        let out = dev.clone();
        self.persist(&st)?;
        Ok(out)
    }

    pub fn remove_device(&self, id: &str) -> Option<Device> {
        let mut st = self.state.lock().unwrap();
        let pos = st.devices.iter().position(|d| d.id == id)?;
        let dev = st.devices.remove(pos);
        let _ = self.persist(&st);
        Some(dev)
    }

    // ---------- 访问记录 ----------

    pub fn log(&self, kind: &str, device: Option<&str>, ip: &str, detail: impl Into<String>) -> AuditEvent {
        let mut audit = self.audit.lock().unwrap();
        let ev = AuditEvent {
            seq: audit.back().map_or(1, |e| e.seq + 1),
            ts: Utc::now(),
            kind: kind.to_string(),
            device: device.map(str::to_string),
            ip: ip.to_string(),
            detail: detail.into(),
        };
        tracing::info!(kind, device, ip, detail = %ev.detail, "远程访问");
        audit.push_back(ev.clone());
        while audit.len() > AUDIT_KEEP {
            audit.pop_front();
        }
        if let Err(e) = self.append_audit(&ev, &audit) {
            tracing::warn!("无法写入访问记录：{e}");
        }
        ev
    }

    fn append_audit(&self, ev: &AuditEvent, all: &VecDeque<AuditEvent>) -> std::io::Result<()> {
        let too_big = std::fs::metadata(&self.audit_file)
            .map(|m| m.len() > AUDIT_FILE_MAX)
            .unwrap_or(false);
        if too_big {
            // 只保留内存中最近的记录
            let mut text = String::new();
            for e in all {
                text.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
                text.push('\n');
            }
            return write_private(&self.audit_file, text.as_bytes());
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        writeln!(f, "{}", serde_json::to_string(ev).map_err(std::io::Error::other)?)
    }

    /// 最近的访问记录（新的在前）；`since` 只返回序号更大的
    pub fn audit(&self, since: u64, limit: usize) -> Vec<AuditEvent> {
        self.audit
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|e| e.seq > since)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn audit_seq(&self) -> u64 {
        self.audit.lock().unwrap().back().map_or(0, |e| e.seq)
    }
}

fn load_audit(path: &Path) -> VecDeque<AuditEvent> {
    let mut out = VecDeque::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            if let Ok(e) = serde_json::from_str::<AuditEvent>(line) {
                out.push_back(e);
                if out.len() > AUDIT_KEEP {
                    out.pop_front();
                }
            }
        }
    }
    out
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    write_atomic(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn clean_name(name: &str) -> String {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    if n.is_empty() {
        "未命名设备".into()
    } else {
        n
    }
}

pub fn random_hex(bytes: usize) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..bytes).map(|_| format!("{:02x}", rng.random::<u8>())).collect()
}

pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_pin_and_audit() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        assert!(s.set_pin("12").is_err());
        assert!(s.set_pin("abcdef").is_err());
        s.set_pin("246810").unwrap();
        assert!(s.has_pin());

        let (dev, token) = s.add_device("  Lucky 的 iPhone\n", "1.2.3.4").unwrap();
        assert_eq!(dev.name, "Lucky 的 iPhone");
        assert_eq!(token.len(), 64);
        // 文件里只有哈希
        let text = std::fs::read_to_string(dir.path().join("remote/devices.json")).unwrap();
        assert!(!text.contains(&token));
        assert!(text.contains(&sha256_hex(&token)));

        assert_eq!(s.find_by_token(&token, 30).unwrap().unwrap().id, dev.id);
        assert!(s.find_by_token(&"0".repeat(64), 30).unwrap().is_none());
        assert!(s.find_by_token("short", 30).unwrap().is_none());

        assert!(s.check_pin(&dev.id, "246810").is_ok());
        for i in 1..MAX_PIN_FAILURES {
            assert!(matches!(s.check_pin(&dev.id, "000000"), Err(PinError::Wrong(n)) if n == MAX_PIN_FAILURES - i));
        }
        assert!(matches!(s.check_pin(&dev.id, "000000"), Err(PinError::Locked)));
        // 锁定后正确 PIN 也不行，直到电脑上解除
        assert!(matches!(s.check_pin(&dev.id, "246810"), Err(PinError::Locked)));
        s.update_device(&dev.id, None, Some(true), true).unwrap();
        assert!(s.check_pin(&dev.id, "246810").is_ok());
        assert!(s.device(&dev.id).unwrap().read_only);

        // 重新打开后状态保留
        let s2 = Store::open(dir.path()).unwrap();
        assert!(s2.has_pin());
        assert_eq!(s2.devices().len(), 1);
        assert!(s2.remove_device(&dev.id).is_some());
        assert!(s2.find_by_token(&token, 30).unwrap().is_none());

        let a = s2.log("pair", Some("x"), "ip", "one");
        let b = s2.log("attach", Some("x"), "ip", "two");
        assert_eq!(b.seq, a.seq + 1);
        assert_eq!(s2.audit(a.seq, 10).len(), 1);
        let s3 = Store::open(dir.path()).unwrap();
        assert_eq!(s3.audit_seq(), b.seq);
    }

    #[test]
    fn expired_devices_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        let (dev, token) = s.add_device("old", "").unwrap();
        s.state.lock().unwrap().devices[0].last_seen = Utc::now() - chrono::Duration::days(31);
        assert!(matches!(s.find_by_token(&token, 30), Err(n) if n == "old"));
        assert!(s.device(&dev.id).is_none());
    }
}
