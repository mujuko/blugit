#![cfg(feature = "test-transport")]

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

struct Harness {
    _directory: TempDir,
    root: PathBuf,
    repository: PathBuf,
    helper_path: String,
    address: String,
    host: Child,
    guest_state: Option<PathBuf>,
}

impl Harness {
    fn new() -> Self {
        Self::with_security(false)
    }

    fn with_security(secure: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let repository = root.join("host repo.git");
        assert!(
            Command::new("git")
                .args(["init", "--bare", "--initial-branch=main"])
                .arg(&repository)
                .output()
                .unwrap()
                .status
                .success()
        );
        let guest_state = secure.then(|| root.join("guest identity"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_blugit"));
        if secure {
            command
                .args(["spike", "__test-host"])
                .arg(&repository)
                .arg("--state-dir")
                .arg(root.join("host identity"))
                .args(["--name", "Test host"]);
        } else {
            command.arg("__test-host").arg(&repository);
        }
        let mut host = command
            .stdout(Stdio::piped())
            .stderr(if secure {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .spawn()
            .unwrap();
        let (code_tx, code_rx) = mpsc::channel();
        if let Some(stderr) = host.stderr.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Some((_, rest)) = line.split_once("CODE=") {
                        let _ = code_tx.send(rest.split_whitespace().next().unwrap().to_owned());
                    } else {
                        eprintln!("{line}");
                    }
                }
            });
        }
        let stdout = host.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let address = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("host did not start")
            .trim()
            .to_string();
        assert!(
            !address.is_empty(),
            "host exited before announcing its test port"
        );
        if let Some(root) = &guest_state {
            let mut join = Command::new(env!("CARGO_BIN_EXE_blugit"))
                .args(["spike", "join", "blugit://AA-BB-CC-DD-EE-FF?secure=1"])
                .arg("--state-dir")
                .arg(root)
                .args(["--name", "Test guest"])
                .env("BLUGIT_TEST_ADDR", &address)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let code = code_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            writeln!(join.stdin.take().unwrap(), "{code}").unwrap();
            let output = join.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "join: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("Joined prototype"));
        }
        let bin = root.join("helper bin");
        fs::create_dir(&bin).unwrap();
        let helper = Path::new(env!("CARGO_BIN_EXE_git-remote-blugit"));
        fs::copy(helper, bin.join(helper.file_name().unwrap())).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let helper_path = std::env::join_paths(paths)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Self {
            _directory: directory,
            root,
            repository,
            helper_path,
            address,
            host,
            guest_state,
        }
    }

    fn git(&self, cwd: &Path, args: &[&OsStr]) -> Output {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(cwd)
            .args(args)
            .env("PATH", &self.helper_path)
            .env("BLUGIT_TEST_ADDR", &self.address)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_AUTHOR_NAME", "Prototype Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Prototype Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_TERMINAL_PROMPT", "0");
        if let Some(root) = &self.guest_state {
            command.env("BLUGIT_STATE_DIR", root);
        } else {
            command.env_remove("BLUGIT_STATE_DIR");
        }
        command.output().unwrap()
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
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.host.kill();
        let _ = self.host.wait();
    }
}

#[test]
fn clone_push_pull_binary_rejection_and_rebase() {
    for (secure, url) in [
        (false, "blugit://AA-BB-CC-DD-EE-FF"),
        (false, "blugit://AA-BB-CC-DD-EE-FF?channel=7"),
        (true, "blugit://AA-BB-CC-DD-EE-FF?secure=1"),
        (true, "blugit://AA-BB-CC-DD-EE-FF?channel=7&secure=1"),
    ] {
        let harness = Harness::with_security(secure);
        harness.ok(
            &harness.root,
            &["-c", "init.defaultBranch=main", "clone", url, "guest one"],
        );
        let first = harness.root.join("guest one");
        let second = harness.root.join("guest two");
        let mut state = 0x11223344u32;
        let binary: Vec<_> = (0..262144)
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
}

#[test]
fn failed_git_service_reaches_the_guest() {
    let harness = Harness::new();
    fs::rename(&harness.repository, harness.root.join("moved.git")).unwrap();
    let output = harness.git(
        &harness.root,
        &[
            OsStr::new("clone"),
            OsStr::new("blugit://AA-BB-CC-DD-EE-FF"),
            OsStr::new("guest"),
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not appear to be a git repository")
    );
}

#[test]
fn secure_host_rejects_unregistered_device_and_plaintext_git() {
    let harness = Harness::with_security(true);
    let output = harness.git(
        &harness.root,
        &[
            OsStr::new("clone"),
            OsStr::new("blugit://AA-BB-CC-DD-EE-FF"),
            OsStr::new("plaintext"),
        ],
    );
    assert!(!output.status.success());
    let state =
        blugit::identity::State::open(harness.root.join("stranger"), Some("Test guest".into()))
            .unwrap();
    let host_state =
        blugit::identity::State::open(harness.root.join("host identity"), None).unwrap();
    state
        .save_host("AA:BB:CC:DD:EE:FF".parse().unwrap(), host_state.peer())
        .unwrap();
    let original = harness.guest_state.as_ref().unwrap();
    fs::rename(original, harness.root.join("saved identity")).unwrap();
    fs::rename(state.root(), original).unwrap();
    let output = harness.git(
        &harness.root,
        &[
            OsStr::new("clone"),
            OsStr::new("blugit://AA-BB-CC-DD-EE-FF?secure=1"),
            OsStr::new("unregistered"),
        ],
    );
    assert!(!output.status.success());
    assert!(!harness.root.join("unregistered/.git/objects/pack").exists());
}
