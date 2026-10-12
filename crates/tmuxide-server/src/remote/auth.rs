//! 配对码、PIN 解锁会话与请求来源相关的纯逻辑，便于单元测试。

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

/// 配对码有效期
pub const PAIR_TTL: Duration = Duration::from_secs(5 * 60);
/// 单个配对码允许的错误次数
pub const PAIR_MAX_FAILURES: u32 = 5;
/// 全局：窗口期内最多允许的错误配对/解锁尝试
const GLOBAL_WINDOW: Duration = Duration::from_secs(10 * 60);
const GLOBAL_MAX_FAILURES: usize = 20;

/// 配对码字母表：去掉容易混淆的 0/O、1/I/L
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

pub struct Pairing {
    pub code: String,
    pub created: Instant,
    pub failures: u32,
}

impl Default for Pairing {
    fn default() -> Self {
        Self::new()
    }
}

impl Pairing {
    pub fn new() -> Pairing {
        use rand::Rng;
        let mut rng = rand::rng();
        let code = (0..8)
            .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
            .collect();
        Pairing {
            code,
            created: Instant::now(),
            failures: 0,
        }
    }

    pub fn expired(&self) -> bool {
        self.created.elapsed() > PAIR_TTL || self.failures >= PAIR_MAX_FAILURES
    }

    pub fn remaining(&self) -> Duration {
        PAIR_TTL.saturating_sub(self.created.elapsed())
    }

    /// 显示用：K7QF-2M9X
    pub fn display(&self) -> String {
        format!("{}-{}", &self.code[..4], &self.code[4..])
    }
}

/// 规范化用户输入的配对码：忽略大小写、空格和 -
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub fn code_matches(expected: &str, input: &str) -> bool {
    let input = normalize_code(input);
    expected.len() == input.len()
        && expected
            .bytes()
            .zip(input.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// 全局失败计数：防止通过大量请求猜配对码或 PIN
#[derive(Default)]
pub struct FailureWindow {
    events: VecDeque<Instant>,
}

impl FailureWindow {
    pub fn record(&mut self) {
        self.events.push_back(Instant::now());
        self.prune();
    }

    pub fn blocked(&mut self) -> bool {
        self.prune();
        self.events.len() >= GLOBAL_MAX_FAILURES
    }

    fn prune(&mut self) {
        while self.events.front().is_some_and(|t| t.elapsed() > GLOBAL_WINDOW) {
            self.events.pop_front();
        }
    }
}

/// PIN 解锁后的会话（只在内存中，重启后需要重新输入 PIN）
pub struct Unlocked {
    pub device_id: String,
    pub last_active: Instant,
}

#[derive(Default)]
pub struct UnlockTable {
    /// 键是会话令牌的 SHA-256
    map: HashMap<String, Unlocked>,
}

impl UnlockTable {
    pub fn insert(&mut self, token_hash: String, device_id: String) {
        self.map.insert(
            token_hash,
            Unlocked {
                device_id,
                last_active: Instant::now(),
            },
        );
    }

    /// 校验并续期；超时或设备不符时删除
    pub fn check(&mut self, token_hash: &str, device_id: &str, idle: Duration) -> bool {
        let ok = match self.map.get_mut(token_hash) {
            Some(u) if u.device_id == device_id && u.last_active.elapsed() <= idle => {
                u.last_active = Instant::now();
                true
            }
            _ => false,
        };
        if !ok {
            self.map.remove(token_hash);
        }
        ok
    }

    pub fn remove(&mut self, token_hash: &str) {
        self.map.remove(token_hash);
    }

    pub fn remove_device(&mut self, device_id: &str) {
        self.map.retain(|_, u| u.device_id != device_id);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// 从 Cookie 头中取出指定名字的值
pub fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name).then_some(v)
    })
}

/// 生成 Set-Cookie。HTTPS 下使用 `__Host-` 前缀（必须 Secure、Path=/、不带 Domain）。
pub fn set_cookie(name: &str, value: &str, secure: bool, max_age: Option<u64>) -> String {
    let mut s = format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Strict",
        cookie_name(name, secure)
    );
    if secure {
        s.push_str("; Secure");
    }
    match max_age {
        Some(age) => s.push_str(&format!("; Max-Age={age}")),
        None if value.is_empty() => s.push_str("; Max-Age=0"),
        None => {}
    }
    s
}

pub fn cookie_name(name: &str, secure: bool) -> String {
    if secure {
        format!("__Host-{name}")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_codes() {
        let p = Pairing::new();
        assert_eq!(p.code.len(), 8);
        assert!(p.code.bytes().all(|b| ALPHABET.contains(&b)));
        assert!(!p.expired());
        let shown = p.display();
        assert!(code_matches(&p.code, &shown));
        assert!(code_matches(&p.code, &shown.to_lowercase().replace('-', " ")));
        assert!(!code_matches(&p.code, "AAAAAAAA"));
        assert!(!code_matches(&p.code, &p.code[..7]));
        let mut q = Pairing::new();
        q.failures = PAIR_MAX_FAILURES;
        assert!(q.expired());
    }

    #[test]
    fn failure_window_blocks() {
        let mut w = FailureWindow::default();
        for _ in 0..GLOBAL_MAX_FAILURES - 1 {
            w.record();
        }
        assert!(!w.blocked());
        w.record();
        assert!(w.blocked());
    }

    #[test]
    fn unlock_sessions_expire_and_bind_device() {
        let mut t = UnlockTable::default();
        t.insert("h1".into(), "dev1".into());
        assert!(t.check("h1", "dev1", Duration::from_secs(60)));
        assert!(!t.check("h1", "dev2", Duration::from_secs(60)), "其他设备不能使用");
        assert!(!t.check("h1", "dev1", Duration::from_secs(60)), "失败后已删除");
        t.insert("h2".into(), "dev1".into());
        assert!(!t.check("h2", "dev1", Duration::ZERO));
        t.insert("h3".into(), "dev1".into());
        t.remove_device("dev1");
        assert!(!t.check("h3", "dev1", Duration::from_secs(60)));
    }

    #[test]
    fn cookies() {
        let h = "a=1; __Host-tmx_d=abc; tmx_s=def";
        assert_eq!(cookie(h, "__Host-tmx_d"), Some("abc"));
        assert_eq!(cookie(h, "tmx_s"), Some("def"));
        assert_eq!(cookie(h, "x"), None);
        let c = set_cookie("tmx_d", "v", true, Some(10));
        assert!(c.starts_with("__Host-tmx_d=v; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=10"));
        let c = set_cookie("tmx_d", "", false, None);
        assert_eq!(c, "tmx_d=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    }
}
