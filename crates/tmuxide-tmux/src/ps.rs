use std::{collections::HashMap, process::Command};

/// 进程表快照，用于取得面板前台程序的完整命令行（tmux 只提供程序名）
pub struct ProcessTable {
    children: HashMap<u32, Vec<(u32, String)>>,
}

impl ProcessTable {
    pub fn load() -> Option<ProcessTable> {
        // ps 是 macOS / Linux 基础系统自带的程序，不属于额外依赖
        let out = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,args="])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(Self::parse(&String::from_utf8_lossy(&out.stdout)))
    }

    fn parse(text: &str) -> ProcessTable {
        let mut children: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let (Some(pid), Some(ppid)) = (it.next(), it.next()) else {
                continue;
            };
            let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else {
                continue;
            };
            let args = it.collect::<Vec<_>>().join(" ");
            children.entry(ppid).or_default().push((pid, args));
        }
        for v in children.values_mut() {
            v.sort_by_key(|(pid, _)| *pid);
        }
        ProcessTable { children }
    }

    /// 面板 shell 启动的第一个子进程的命令行
    pub fn foreground_args(&self, shell_pid: u32) -> Option<String> {
        self.children.get(&shell_pid)?.first().map(|(_, a)| a.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_child_args() {
        let t = ProcessTable::parse(
            "  100     1 -zsh\n  205   100 cargo watch -x test\n  300   205 rustc --edition 2021\n",
        );
        assert_eq!(t.foreground_args(100).as_deref(), Some("cargo watch -x test"));
        assert_eq!(t.foreground_args(999), None);
    }
}
