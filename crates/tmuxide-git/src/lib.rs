//! Git 适配层：基于静态内嵌的 libgit2，不调用系统 `git` 命令。

use std::{
    cell::Cell,
    path::{Path, PathBuf},
};

use git2::{
    build::CheckoutBuilder, Cred, CredentialType, Direction, FetchOptions, IndexAddOption, ObjectType, Oid,
    PushOptions, RemoteCallbacks, Repository, RepositoryInitOptions, Signature, Sort,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("{0}")]
    Git(#[from] git2::Error),
    #[error("尚未配置远程仓库地址")]
    NoRemote,
    #[error("合并出现冲突：{0}。请在其他电脑上处理后再同步")]
    Conflict(String),
    #[error("远程拒绝了推送：{0}")]
    Rejected(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, GitError>;

/// 认证方式
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Auth {
    /// 不认证（本地路径或公开只读仓库）
    #[default]
    None,
    /// 本机私钥文件
    SshKey { path: PathBuf, passphrase: Option<String> },
    /// ssh-agent 中已加载的密钥
    SshAgent,
    /// HTTPS 用户名 + Token
    HttpsToken { username: String, token: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub id: String,
    pub short: String,
    pub summary: String,
    pub time: i64,
    pub author: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub branch: String,
    pub has_remote: bool,
    pub ahead: usize,
    pub behind: usize,
    pub head: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    /// " "、"+" 或 "-"
    pub tag: char,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
}

pub struct SnapshotRepo {
    repo: Repository,
    branch: String,
}

impl SnapshotRepo {
    /// 打开仓库，不存在时初始化
    pub fn open_or_init(dir: &Path, branch: &str) -> Result<SnapshotRepo> {
        let repo = match Repository::open(dir) {
            Ok(r) => r,
            Err(_) => {
                std::fs::create_dir_all(dir).map_err(|e| GitError::Other(e.to_string()))?;
                let mut opts = RepositoryInitOptions::new();
                opts.initial_head(branch);
                Repository::init_opts(dir, &opts)?
            }
        };
        Ok(SnapshotRepo {
            repo,
            branch: branch.to_string(),
        })
    }

    pub fn workdir(&self) -> &Path {
        self.repo.workdir().expect("非 bare 仓库")
    }

    pub fn set_remote(&self, url: &str) -> Result<()> {
        let url = url.trim();
        match self.repo.find_remote("origin") {
            Ok(r) if r.url() == Some(url) => {}
            Ok(_) if url.is_empty() => self.repo.remote_delete("origin")?,
            Ok(_) => self.repo.remote_set_url("origin", url)?,
            Err(_) if url.is_empty() => {}
            Err(_) => {
                self.repo.remote("origin", url)?;
            }
        }
        Ok(())
    }

    fn has_remote(&self) -> bool {
        self.repo.find_remote("origin").is_ok()
    }

    /// 提交工作区的所有变化；没有变化时返回 None
    pub fn commit_all(&self, message: &str, name: &str, email: &str) -> Result<Option<String>> {
        let mut index = self.repo.index()?;
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"].iter(), None)?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let parent = self.head_commit()?;
        if let Some(p) = &parent {
            if p.tree_id() == tree_id {
                return Ok(None);
            }
        }
        let tree = self.repo.find_tree(tree_id)?;
        let sig = signature(name, email)?;
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = self.repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
        Ok(Some(oid.to_string()))
    }

    fn head_commit(&self) -> Result<Option<git2::Commit<'_>>> {
        match self.repo.head() {
            Ok(h) => Ok(Some(h.peel_to_commit()?)),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch || e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn remote_ref(&self) -> String {
        format!("refs/remotes/origin/{}", self.branch)
    }

    pub fn status(&self) -> Result<SyncStatus> {
        let head = self.head_commit()?.map(|c| c.id());
        let mut st = SyncStatus {
            branch: self.branch.clone(),
            has_remote: self.has_remote(),
            head: head.map(|h| h.to_string()),
            ..Default::default()
        };
        if let (Some(local), Ok(remote)) = (head, self.repo.refname_to_id(&self.remote_ref())) {
            let (a, b) = self.repo.graph_ahead_behind(local, remote)?;
            st.ahead = a;
            st.behind = b;
        } else if head.is_some() && st.has_remote {
            // 远程分支尚不存在：本地所有提交都待推送
            let mut walk = self.repo.revwalk()?;
            walk.push_head()?;
            st.ahead = walk.count();
        }
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        st.dirty = !self.repo.statuses(Some(&mut opts))?.is_empty();
        Ok(st)
    }

    /// 测试能否连接并认证远程仓库。
    ///
    /// 不调用 `list()`：git2 0.20 在空远程仓库上返回空指针会触发 UB 检查。
    pub fn test_connection(&self, auth: &Auth) -> Result<()> {
        let mut remote = self.repo.find_remote("origin").map_err(|_| GitError::NoRemote)?;
        let mut conn = remote.connect_auth(Direction::Fetch, Some(callbacks(auth)), None)?;
        if !conn.remote().connected() {
            return Err(GitError::Other("无法连接远程仓库".into()));
        }
        Ok(())
    }

    /// 拉取远程分支并合并（快进或合并提交）。返回是否有更新
    pub fn pull(&self, auth: &Auth, name: &str, email: &str) -> Result<bool> {
        let mut remote = self.repo.find_remote("origin").map_err(|_| GitError::NoRemote)?;
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(callbacks(auth));
        let refspec = format!("+refs/heads/{0}:refs/remotes/origin/{0}", self.branch);
        remote.fetch(&[&refspec], Some(&mut fo), None)?;
        let Ok(remote_id) = self.repo.refname_to_id(&self.remote_ref()) else {
            return Ok(false); // 远程是空仓库
        };
        let remote_commit = self.repo.find_annotated_commit(remote_id)?;
        let (analysis, _) = self.repo.merge_analysis(&[&remote_commit])?;
        let local_ref = format!("refs/heads/{}", self.branch);
        if analysis.is_up_to_date() {
            return Ok(false);
        }
        if analysis.is_unborn() || analysis.is_fast_forward() {
            match self.repo.find_reference(&local_ref) {
                Ok(mut r) => {
                    r.set_target(remote_id, "tmuxide: fast-forward")?;
                }
                Err(_) => {
                    self.repo
                        .reference(&local_ref, remote_id, true, "tmuxide: init from remote")?;
                }
            }
            self.repo.set_head(&local_ref)?;
            self.repo.checkout_head(Some(CheckoutBuilder::new().force()))?;
            return Ok(true);
        }
        // 真正的合并：每台电脑写各自目录，正常不会冲突
        let local = self
            .head_commit()?
            .ok_or_else(|| GitError::Other("本地没有提交".into()))?;
        let theirs = self.repo.find_commit(remote_id)?;
        let mut index = self.repo.merge_commits(&local, &theirs, None)?;
        if index.has_conflicts() {
            let files: Vec<String> = index
                .conflicts()?
                .flatten()
                .filter_map(|c| c.our.or(c.their).map(|e| String::from_utf8_lossy(&e.path).into_owned()))
                .collect();
            return Err(GitError::Conflict(files.join(", ")));
        }
        let tree_id = index.write_tree_to(&self.repo)?;
        let tree = self.repo.find_tree(tree_id)?;
        let sig = signature(name, email)?;
        self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "merge: 同步其他电脑的快照",
            &tree,
            &[&local, &theirs],
        )?;
        self.repo.checkout_head(Some(CheckoutBuilder::new().force()))?;
        Ok(true)
    }

    pub fn push(&self, auth: &Auth) -> Result<()> {
        let mut remote = self.repo.find_remote("origin").map_err(|_| GitError::NoRemote)?;
        let rejected: std::cell::RefCell<Option<String>> = Default::default();
        {
            let mut cb = callbacks(auth);
            cb.push_update_reference(|_, status| {
                if let Some(s) = status {
                    *rejected.borrow_mut() = Some(s.to_string());
                }
                Ok(())
            });
            let mut po = PushOptions::new();
            po.remote_callbacks(cb);
            let spec = format!("refs/heads/{0}:refs/heads/{0}", self.branch);
            remote.push(&[&spec], Some(&mut po))?;
        }
        if let Some(r) = rejected.into_inner() {
            return Err(GitError::Rejected(r));
        }
        // 推送成功后更新远程跟踪分支，便于计算领先/落后
        if let Some(head) = self.head_commit()? {
            self.repo
                .reference(&self.remote_ref(), head.id(), true, "tmuxide: push")?;
        }
        Ok(())
    }

    /// 修改过指定文件的提交，新的在前
    pub fn history(&self, path: &str, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut out = Vec::new();
        if self.head_commit()?.is_none() {
            return Ok(out);
        }
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        let p = Path::new(path);
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            let this = commit.tree()?.get_path(p).ok().map(|e| e.id());
            if this.is_none() {
                continue;
            }
            let changed = if commit.parent_count() == 0 {
                true
            } else {
                commit
                    .parents()
                    .all(|parent| parent.tree().ok().and_then(|t| t.get_path(p).ok()).map(|e| e.id()) != this)
            };
            if changed {
                out.push(commit_info(&commit));
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// 读取某次提交中的文件内容
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Option<String>> {
        let commit = self
            .repo
            .find_commit(Oid::from_str(rev).or_else(|_| self.repo.revparse_single(rev).map(|o| o.id()))?)?;
        let tree = commit.tree()?;
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(None);
        };
        if entry.kind() != Some(ObjectType::Blob) {
            return Ok(None);
        }
        let blob = self.repo.find_blob(entry.id())?;
        Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
    }

    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut out = Vec::new();
        if self.head_commit()?.is_none() {
            return Ok(out);
        }
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        for oid in walk.take(limit) {
            out.push(commit_info(&self.repo.find_commit(oid?)?));
        }
        Ok(out)
    }
}

/// 创建一个空的 bare 仓库，可放在 NAS 或同步盘上作为远程仓库
pub fn init_bare(dir: &Path, branch: &str) -> Result<()> {
    let mut opts = RepositoryInitOptions::new();
    opts.bare(true).initial_head(branch);
    Repository::init_opts(dir, &opts)?;
    Ok(())
}

/// 逐行对比两个文本
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    similar::TextDiff::from_lines(old, new)
        .iter_all_changes()
        .map(|c| DiffLine {
            tag: match c.tag() {
                similar::ChangeTag::Equal => ' ',
                similar::ChangeTag::Insert => '+',
                similar::ChangeTag::Delete => '-',
            },
            old_no: c.old_index().map(|i| i + 1),
            new_no: c.new_index().map(|i| i + 1),
            text: c.value().trim_end_matches('\n').to_string(),
        })
        .collect()
}

fn commit_info(c: &git2::Commit) -> CommitInfo {
    let id = c.id().to_string();
    CommitInfo {
        short: id[..7].to_string(),
        id,
        summary: c.summary().unwrap_or("").to_string(),
        time: c.time().seconds(),
        author: c.author().name().unwrap_or("").to_string(),
    }
}

fn signature(name: &str, email: &str) -> Result<Signature<'static>> {
    let name = if name.trim().is_empty() { "TmuxIDE" } else { name.trim() };
    let email = if email.trim().is_empty() {
        "tmuxide@localhost"
    } else {
        email.trim()
    };
    Ok(Signature::now(name, email)?)
}

fn callbacks(auth: &Auth) -> RemoteCallbacks<'_> {
    let mut cb = RemoteCallbacks::new();
    let attempts = Cell::new(0u32);
    cb.credentials(move |url, username_from_url, allowed| {
        attempts.set(attempts.get() + 1);
        // libgit2 认证失败时会反复回调，限制次数避免死循环
        if attempts.get() > 3 {
            return Err(git2::Error::from_str("认证失败，请检查密钥或 Token"));
        }
        let user = username_from_url.unwrap_or("git");
        match auth {
            Auth::SshKey { path, passphrase } if allowed.contains(CredentialType::SSH_KEY) => {
                Cred::ssh_key(user, None, &expand_tilde(path), passphrase.as_deref())
            }
            Auth::SshAgent if allowed.contains(CredentialType::SSH_KEY) => Cred::ssh_key_from_agent(user),
            Auth::HttpsToken { username, token } if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) => {
                let u = if username.is_empty() { "git" } else { username.as_str() };
                Cred::userpass_plaintext(u, token)
            }
            _ if allowed.contains(CredentialType::USERNAME) => Cred::username(user),
            _ => Err(git2::Error::from_str(&format!("{url} 需要的认证方式与设置不一致"))),
        }
    });
    cb.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificatePassthrough));
    cb
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(rest) = p.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn commit_push_pull_between_two_machines() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("remote.git");
        Repository::init_bare(&bare).unwrap();
        let url = bare.display().to_string();

        let a = SnapshotRepo::open_or_init(&tmp.path().join("a"), "main").unwrap();
        a.set_remote(&url).unwrap();
        write(a.workdir(), "hosts/a/sessions/dev.toml", "name = \"dev\"\n");
        let c1 = a.commit_all("snapshot(dev)", "A", "a@x").unwrap();
        assert!(c1.is_some());
        assert_eq!(a.commit_all("noop", "A", "a@x").unwrap(), None);
        assert_eq!(a.status().unwrap().ahead, 1);
        a.push(&Auth::None).unwrap();
        assert_eq!(a.status().unwrap().ahead, 0);

        let b = SnapshotRepo::open_or_init(&tmp.path().join("b"), "main").unwrap();
        b.set_remote(&url).unwrap();
        assert!(b.pull(&Auth::None, "B", "b@x").unwrap());
        assert!(b.workdir().join("hosts/a/sessions/dev.toml").exists());

        // 两台电脑各自写入自己的目录后同步，不应冲突
        write(b.workdir(), "hosts/b/sessions/ops.toml", "name = \"ops\"\n");
        b.commit_all("snapshot(ops)", "B", "b@x").unwrap();
        b.push(&Auth::None).unwrap();
        write(
            a.workdir(),
            "hosts/a/sessions/dev.toml",
            "name = \"dev\"\nroot = \"~\"\n",
        );
        a.commit_all("snapshot(dev) 2", "A", "a@x").unwrap();
        assert!(a.pull(&Auth::None, "A", "a@x").unwrap());
        assert!(a.workdir().join("hosts/b/sessions/ops.toml").exists());
        a.push(&Auth::None).unwrap();

        let hist = a.history("hosts/a/sessions/dev.toml", 10).unwrap();
        assert_eq!(hist.len(), 2);
        let old = a.file_at(&hist[1].id, "hosts/a/sessions/dev.toml").unwrap().unwrap();
        let new = a.file_at(&hist[0].id, "hosts/a/sessions/dev.toml").unwrap().unwrap();
        let d = diff_lines(&old, &new);
        assert!(d.iter().any(|l| l.tag == '+' && l.text == "root = \"~\""));
        a.test_connection(&Auth::None).unwrap();
        // 空的远程仓库也能测试连接
        let empty = tmp.path().join("empty.git");
        Repository::init_bare(&empty).unwrap();
        let c = SnapshotRepo::open_or_init(&tmp.path().join("c"), "main").unwrap();
        c.set_remote(&empty.display().to_string()).unwrap();
        c.test_connection(&Auth::None).unwrap();
    }
}
