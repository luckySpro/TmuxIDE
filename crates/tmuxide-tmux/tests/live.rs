//! 需要真实 tmux 的集成测试：用临时 socket，完成后关闭 server。
//! 没有可用 tmux 时自动跳过。

use tmuxide_tmux::{Tmux, TmuxOptions};

fn tmux(tag: &str) -> Option<(Tmux, tempfile::TempDir)> {
    let dir = tempfile::tempdir().unwrap();
    let opts = TmuxOptions {
        data_dir: dir.path().to_path_buf(),
        use_system: false,
        socket: format!("tmuxide-test-{tag}-{}", std::process::id()),
    };
    match Tmux::new(&opts) {
        Ok(t) => Some((t, dir)),
        Err(e) => {
            eprintln!("跳过：{e}");
            None
        }
    }
}

#[test]
fn save_kill_restore_roundtrip() {
    let Some((t, dir)) = tmux("roundtrip") else { return };
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();

    t.new_session("demo", Some(&cwd)).unwrap();
    t.new_window("demo", Some("second")).unwrap();
    let s = t.session("demo").unwrap();
    let w2 = &s.windows[1];
    t.run(&["split-window", "-t", &w2.id, "-c", cwd.to_str().unwrap()])
        .unwrap();
    t.run(&["send-keys", "-t", &w2.panes[0].id, "-l", "sleep 300"]).unwrap();
    t.run(&["send-keys", "-t", &w2.panes[0].id, "Enter"]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(800));

    let snap = t.snapshot("demo", "test-host", "2026-10-10T00:00:00Z").unwrap();
    assert_eq!(snap.windows.len(), 2);
    assert_eq!(snap.windows[1].name, "second");
    assert_eq!(snap.windows[1].panes.len(), 2);
    assert!(
        snap.windows[1]
            .panes
            .iter()
            .any(|p| p.cmd.as_deref() == Some("sleep 300")),
        "{snap:?}"
    );

    let text = snap.to_toml().unwrap();
    t.kill_session("demo").unwrap();
    assert!(!t.has_session("demo"));

    let snap = tmuxide_core::SessionSnapshot::from_toml(&text).unwrap();
    let report = t.restore(&snap, "demo-2", true).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let restored = t.session("demo-2").unwrap();
    assert_eq!(restored.windows.len(), 2);
    assert_eq!(restored.windows[1].name, "second");
    assert_eq!(restored.windows[1].panes.len(), 2);
    assert_eq!(
        restored.windows[1].panes[0].cwd,
        cwd.canonicalize().unwrap().display().to_string()
    );

    let _ = t.run(&["kill-server"]);
}

#[test]
fn rejects_bad_names_and_duplicates() {
    let Some((t, _dir)) = tmux("names") else { return };
    assert!(t.new_session("a:b", None).is_err());
    t.new_session("dup", None).unwrap();
    assert!(t.new_session("dup", None).is_err());
    t.rename_session("dup", "renamed").unwrap();
    assert!(t.has_session("renamed"));
    let _ = t.run(&["kill-server"]);
}
