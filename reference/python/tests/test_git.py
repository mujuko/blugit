"""Run real Git and the real helper/host framing with only the radio replaced."""

import asyncio
import os
import shlex
import shutil
import subprocess
import sys
import threading
from pathlib import Path

import pytest
from loopback_helper import StreamChannel

from blugit.host import serve_git

URL = "blugit://AA-BB-CC-DD-EE-FF"


def git(*args, cwd=None, env=None, check=True):
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        env=env,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
    )


@pytest.fixture
def host(tmp_path):
    repository = tmp_path / "host repo.git"
    git("init", "--bare", "--initial-branch=main", str(repository))
    ready = threading.Event()
    state = {}
    errors = []

    async def run():
        tasks = set()

        async def handle(reader, writer):
            task = asyncio.current_task()
            tasks.add(task)
            try:
                await serve_git(StreamChannel(reader, writer), repository)
            except Exception as exc:
                errors.append(exc)
            finally:
                writer.close()
                await writer.wait_closed()
                tasks.discard(task)

        server = await asyncio.start_server(handle, "127.0.0.1", 0)
        state.update(loop=asyncio.get_running_loop(), stop=asyncio.Event())
        state["port"] = server.sockets[0].getsockname()[1]
        ready.set()
        async with server:
            await state["stop"].wait()
        if tasks:
            await asyncio.wait_for(asyncio.gather(*tasks), 5)

    def worker():
        try:
            asyncio.run(run())
        except Exception as exc:
            errors.append(exc)
            ready.set()

    external = None
    if rust_host := os.environ.get("BLUGIT_RUST_HOST"):
        external = subprocess.Popen(
            [rust_host, "__test-host", str(repository)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        state["port"] = int(external.stdout.readline().strip().rsplit(":", 1)[1])
    else:
        thread = threading.Thread(target=worker, daemon=True)
        thread.start()
        assert ready.wait(10), "test server did not start"
        assert not errors

    # Git for Windows understands executable shell helpers, as does Git on macOS.
    helper_dir = tmp_path / "helpers"
    helper_dir.mkdir()
    adapter = Path(__file__).with_name("loopback_helper.py").as_posix()
    executable = Path(sys.executable).as_posix()
    launcher = helper_dir / "git-remote-blugit"
    launcher.write_text(
        f'#!/bin/sh\nexec {shlex.quote(executable)} {shlex.quote(adapter)} "$@"\n',
        encoding="utf-8",
        newline="\n",
    )
    launcher.chmod(0o755)
    if rust_helper := os.environ.get("BLUGIT_RUST_HELPER"):
        launcher.unlink()
        shutil.copy2(rust_helper, helper_dir / Path(rust_helper).name)
    env = {
        **os.environ,
        "PATH": str(helper_dir) + os.pathsep + os.environ["PATH"],
        "BLUGIT_TEST_PORT": str(state["port"]),
        "BLUGIT_TEST_ADDR": f"127.0.0.1:{state['port']}",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_AUTHOR_NAME": "Prototype Test",
        "GIT_AUTHOR_EMAIL": "test@example.invalid",
        "GIT_COMMITTER_NAME": "Prototype Test",
        "GIT_COMMITTER_EMAIL": "test@example.invalid",
        "GIT_TERMINAL_PROMPT": "0",
    }
    try:
        yield repository, env
    finally:
        if external is not None:
            external.terminate()
            external.wait(timeout=10)
        else:
            state["loop"].call_soon_threadsafe(state["stop"].set)
            thread.join(10)
            assert not thread.is_alive(), "Git session failed to clean up"
        assert not errors, errors


def test_clone_push_pull_and_rejected_push(host, tmp_path):
    repository, env = host
    guest = tmp_path / "guest one"
    other = tmp_path / "guest two"
    git("-c", "init.defaultBranch=main", "clone", URL, str(guest), env=env)
    binary = bytes(range(256)) + os.urandom(256 * 1024)
    (guest / "binary.dat").write_bytes(binary)
    git("add", ".", cwd=guest, env=env)
    git("commit", "-m", "initial binary data", cwd=guest, env=env)
    git("push", "-u", "origin", "main", cwd=guest, env=env)
    git("clone", URL, str(other), env=env)
    assert (other / "binary.dat").read_bytes() == binary

    (other / "from-other.txt").write_text("second guest\n", encoding="utf-8")
    git("add", ".", cwd=other, env=env)
    git("commit", "-m", "other guest change", cwd=other, env=env)
    git("push", cwd=other, env=env)
    git("pull", "--ff-only", cwd=guest, env=env)
    assert (guest / "from-other.txt").read_text() == "second guest\n"
    git("pull", "--ff-only", cwd=guest, env=env)  # Already up to date.

    (other / "new.txt").write_text("remote advance\n", encoding="utf-8")
    git("add", ".", cwd=other, env=env)
    git("commit", "-m", "remote advance", cwd=other, env=env)
    git("push", cwd=other, env=env)
    remote_head = git("rev-parse", "main", cwd=repository).stdout
    (guest / "local.txt").write_text("local commit\n", encoding="utf-8")
    git("add", ".", cwd=guest, env=env)
    git("commit", "-m", "local advance", cwd=guest, env=env)
    rejected = git("push", cwd=guest, env=env, check=False)
    assert rejected.returncode != 0
    assert b"rejected" in rejected.stderr
    assert git("rev-parse", "main", cwd=repository).stdout == remote_head
    git("pull", "--rebase", cwd=guest, env=env)
    git("push", cwd=guest, env=env)
    assert (
        git("rev-parse", "HEAD", cwd=guest).stdout
        == git("rev-parse", "main", cwd=repository).stdout
    )
    assert git("--git-dir", str(repository), "fsck", "--full").returncode == 0


def test_git_service_failure_reaches_guest(host, tmp_path):
    repository, env = host
    # The repo was valid at startup, but is no longer available to the host process.
    repository.rename(repository.with_name("moved.git"))
    result = git("clone", URL, str(tmp_path / "guest"), env=env, check=False)
    assert result.returncode != 0
    assert b"does not appear to be a git repository" in result.stderr
