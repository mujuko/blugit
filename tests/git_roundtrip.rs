#![cfg(feature = "test-transport")]
//! Replace only the radio: real CLI registration, remote helper and Git processes.
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};
use tempfile::TempDir;

struct RunningHost {
    child: Child,
    address: String,
    code: String,
    codes: mpsc::Receiver<String>,
}
impl RunningHost {
    fn start(repository: &Path, state: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_blugit"))
            .arg("--state-dir")
            .arg(state)
            .arg("__test-host")
            .arg(repository)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (code_tx, codes) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some((_, tail)) = line.split_once("CODE=") {
                    let _ = code_tx.send(tail.split_whitespace().next().unwrap().to_owned());
                } else {
                    eprintln!("{line}");
                }
            }
        });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let lines: Vec<_> = BufReader::new(stdout)
                .lines()
                .take(2)
                .map_while(Result::ok)
                .collect();
            let _ = tx.send(lines);
        });
        let mut host = Self {
            child,
            address: String::new(),
            code: String::new(),
            codes,
        };
        let lines = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("host startup timed out");
        assert_eq!(lines.len(), 2, "host failed to start");
        host.address = lines[0].strip_prefix("TEST_ADDR=").unwrap().to_owned();
        host.code = lines[1]
            .strip_prefix("CONNECTION_CODE=")
            .unwrap()
            .to_owned();
        host
    }
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for RunningHost {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Harness {
    _directory: TempDir,
    root: PathBuf,
    repository: PathBuf,
    host_state: PathBuf,
    guest_state: PathBuf,
    helper_path: std::ffi::OsString,
    host: RunningHost,
}
impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let repository = root.join("host repo.git");
        Self::init_bare(&repository);
        let host_state = root.join("host state");
        let guest_state = root.join("guest state");
        let host = RunningHost::start(&repository, &host_state);
        let bin = root.join("helper bin");
        fs::create_dir(&bin).unwrap();
        let helper = Path::new(env!("CARGO_BIN_EXE_git-remote-blugit"));
        fs::copy(helper, bin.join(helper.file_name().unwrap())).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let helper_path = std::env::join_paths(paths).unwrap();
        Self {
            _directory: directory,
            root,
            repository,
            host_state,
            guest_state,
            helper_path,
            host,
        }
    }
    fn init_bare(path: &Path) {
        assert!(
            Command::new("git")
                .args(["init", "--bare", "--initial-branch=main"])
                .arg(path)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    fn join(&self) {
        let output = self.enroll("demo", false);
        assert!(
            output.status.success(),
            "join: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("git clone blugit://demo"));
    }
    fn enroll(&self, name: &str, wrong_code: bool) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_blugit"))
            .arg("--state-dir")
            .arg(&self.guest_state)
            .arg("join")
            .arg(&self.host.code)
            .arg(name)
            .env("BLUGIT_TEST_ADDR", &self.host.address)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let code = self
            .host
            .codes
            .recv_timeout(Duration::from_secs(10))
            .expect("host did not issue a code");
        let code = if wrong_code {
            format!("{:06}", (code.parse::<u32>().unwrap() + 1) % 1_000_000)
        } else {
            code
        };
        writeln!(child.stdin.take().unwrap(), "{code}").unwrap();
        child.wait_with_output().unwrap()
    }
    fn git(&self, cwd: &Path, args: &[&OsStr]) -> Output {
        let mut c = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                c.env_remove(key);
            }
        }
        c.current_dir(cwd)
            .args(args)
            .env("PATH", &self.helper_path)
            .env("BLUGIT_HOME", &self.guest_state)
            .env("BLUGIT_TEST_ADDR", &self.host.address)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_AUTHOR_NAME", "Blugit Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Blugit Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_TERMINAL_PROMPT", "0");
        c.output().unwrap()
    }
    fn ok(&self, cwd: &Path, args: &[&str]) -> Vec<u8> {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        let output = self.git(cwd, &args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn host_cli(&self, args: &[&OsStr]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_blugit"))
            .arg("--state-dir")
            .arg(&self.host_state)
            .args(args)
            .output()
            .unwrap()
    }
}

#[test]
fn wrong_confirmation_and_bad_connection_code_never_register_a_name() {
    let mut h = Harness::new();
    let mut bad = h.host.code.clone().into_bytes();
    let last = bad.last_mut().unwrap();
    *last = if *last == b'0' { b'1' } else { b'0' };
    let invalid = Command::new(env!("CARGO_BIN_EXE_blugit"))
        .args(["--state-dir"])
        .arg(&h.guest_state)
        .args(["join", &String::from_utf8(bad).unwrap(), "demo"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("checksum mismatch"));
    assert!(h.host.codes.try_recv().is_err());
    let failed = h.enroll("demo", true);
    assert!(!failed.status.success());
    assert!(!h.guest_state.join("aliases/alias-demo.json").exists());
    assert!(!h.guest_state.join("hosts").exists());
    h.host.stop();
    h.host = RunningHost::start(&h.repository, &h.host_state);
    h.join();
    h.ok(&h.root, &["clone", "blugit://DEMO", "work"]);
}

#[test]
fn existing_name_keeps_its_repository_when_host_switches_shares() {
    let mut h = Harness::new();
    h.join();
    let alias_path = h.guest_state.join("aliases/alias-demo.json");
    let before = fs::read(&alias_path).unwrap();
    h.host.stop();
    let other = h.root.join("other.git");
    Harness::init_bare(&other);
    h.host = RunningHost::start(&other, &h.host_state);
    let rejected = Command::new(env!("CARGO_BIN_EXE_blugit"))
        .arg("--state-dir")
        .arg(&h.guest_state)
        .args(["join", &h.host.code, "Demo"])
        .env("BLUGIT_TEST_ADDR", &h.host.address)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("share ID does not match"));
    assert!(h.host.codes.try_recv().is_err());
    assert_eq!(fs::read(&alias_path).unwrap(), before);
    let output = h.enroll("other", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    h.ok(&h.root, &["clone", "blugit://other", "other-work"]);
    assert_eq!(fs::read(&alias_path).unwrap(), before);
    let wrong = h.git(
        &h.root,
        &[
            OsStr::new("clone"),
            OsStr::new("blugit://demo"),
            OsStr::new("wrong-work"),
        ],
    );
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("share ID does not match"));
}

#[test]
fn restart_revocation_and_repository_switch_preserve_boundaries() {
    let mut h = Harness::new();
    h.join();
    let url = "blugit://demo";
    let code = h.host.code.clone();
    h.ok(&h.root, &["clone", url, "work"]);
    h.host.stop();
    h.host = RunningHost::start(&h.repository, &h.host_state);
    assert_eq!(
        h.host.code, code,
        "restart must not change the connection code"
    );
    h.ok(&h.root.join("work"), &["fetch"]);
    let peers = h.host_cli(&[OsStr::new("peers"), h.repository.as_os_str()]);
    assert!(peers.status.success());
    let text = String::from_utf8(peers.stdout).unwrap();
    let id = text.split_whitespace().next().unwrap();
    assert_eq!(id.len(), 64);
    let revoked = h.host_cli(&[
        OsStr::new("revoke"),
        h.repository.as_os_str(),
        OsStr::new(id),
    ]);
    assert!(revoked.status.success());
    let denied = h.git(&h.root.join("work"), &[OsStr::new("fetch")]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("not registered"));
    assert!(h.repository.join("HEAD").exists());
    h.host.stop();
    let other = h.root.join("other.git");
    Harness::init_bare(&other);
    h.host = RunningHost::start(&other, &h.host_state);
    assert_eq!(
        h.host.code, code,
        "routing code identifies the host, not its repository"
    );
    let wrong = h.git(&h.root.join("work"), &[OsStr::new("fetch")]);
    assert!(!wrong.status.success());
    // The new share has no enrollment for this guest: SSH refuses it before exec.
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("not registered"));
    let new_share = h.git(
        &h.root,
        &[
            OsStr::new("clone"),
            OsStr::new("blugit://other"),
            OsStr::new("unregistered"),
        ],
    );
    assert!(!new_share.status.success());
}

#[test]
fn another_host_cannot_replace_the_running_share() {
    let h = Harness::new();
    let other = h.root.join("other.git");
    Harness::init_bare(&other);
    let result = h.host_cli(&[OsStr::new("__test-host"), other.as_os_str()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("another blugit process"));
    assert!(!other.join(".blugit-share.json").exists());
}

#[test]
fn clone_push_pull_binary_rejection_and_rebase() {
    let harness = Harness::new();
    harness.join();
    let url = "blugit://demo";
    harness.ok(
        &harness.root,
        &["-c", "init.defaultBranch=main", "clone", url, "guest one"],
    );
    let first = harness.root.join("guest one");
    let second = harness.root.join("guest two");
    let mut state = 0x11223344u32;
    // Exceed SSH's initial 2 MiB window to exercise flow control in both directions.
    let binary: Vec<_> = (0..3 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    fs::write(first.join("binary.dat"), &binary).unwrap();
    harness.ok(&first, &["add", "."]);
    harness.ok(&first, &["commit", "-m", "initial binary data"]);
    harness.ok(&first, &["push", "-u", "origin", "main"]);
    harness.ok(&harness.root, &["clone", url, "guest two"]);
    assert_eq!(fs::read(second.join("binary.dat")).unwrap(), binary);
    fs::write(second.join("from-other.txt"), b"second guest\n").unwrap();
    harness.ok(&second, &["add", "."]);
    harness.ok(&second, &["commit", "-m", "other guest change"]);
    harness.ok(&second, &["push"]);
    harness.ok(&first, &["pull", "--ff-only"]);
    assert_eq!(
        fs::read(first.join("from-other.txt")).unwrap(),
        b"second guest\n"
    );
    harness.ok(&first, &["pull", "--ff-only"]);

    fs::write(second.join("remote.txt"), b"remote advance\n").unwrap();
    harness.ok(&second, &["add", "."]);
    harness.ok(&second, &["commit", "-m", "remote advance"]);
    harness.ok(&second, &["push"]);
    let head = harness.ok(&harness.repository, &["rev-parse", "main"]);
    fs::write(first.join("local.txt"), b"local advance\n").unwrap();
    harness.ok(&first, &["add", "."]);
    harness.ok(&first, &["commit", "-m", "local advance"]);
    let rejected = harness.git(&first, &[OsStr::new("push")]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("rejected"));
    assert_eq!(
        harness.ok(&harness.repository, &["rev-parse", "main"]),
        head
    );
    harness.ok(&first, &["pull", "--rebase"]);
    harness.ok(&first, &["push"]);
    assert_eq!(
        harness.ok(&first, &["rev-parse", "HEAD"]),
        harness.ok(&harness.repository, &["rev-parse", "main"])
    );
    harness.ok(&harness.repository, &["fsck", "--full"]);
}
