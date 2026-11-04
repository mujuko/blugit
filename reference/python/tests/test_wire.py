import asyncio
import subprocess

import pytest
from loopback_helper import StreamChannel

from blugit.helper import parse_url
from blugit.host import serve_git
from blugit.wire import HEADER, MAX_FRAME, Kind, Peer


@pytest.mark.parametrize(
    "url",
    [
        "blugit://AA-BB-CC-DD-EE-FF",
        "blugit://aa:bb:cc:dd:ee:ff/",
    ],
)
def test_address(url):
    assert parse_url(url) == ("AA:BB:CC:DD:EE:FF", None)


@pytest.mark.parametrize(
    "url",
    [
        "http://AA-BB-CC-DD-EE-FF",
        "blugit://hostname/repo",
        "blugit://AA-BB-CC-DD-EE-FF/../x",
        "blugit://AA-BB-CC-DD-EE-FF?x",
        "blugit://AA-BB-CC-DD-EE-FF#x",
    ],
)
def test_invalid_address(url):
    with pytest.raises(ValueError):
        parse_url(url)


def test_disconnect_cleans_up_git(tmp_path):
    subprocess.run(["git", "init", "--bare", str(tmp_path)], check=True, capture_output=True)

    async def run():
        finished = asyncio.Event()
        errors = []

        async def handle(reader, writer):
            try:
                await serve_git(StreamChannel(reader, writer), tmp_path)
                errors.append("expected a disconnect error")
            except (asyncio.IncompleteReadError, ConnectionError):
                pass
            except BaseException as exc:
                errors.append(exc)
            finally:
                writer.close()
                await writer.wait_closed()
                finished.set()

        async with await asyncio.start_server(handle, "127.0.0.1", 0) as server:
            reader, writer = await asyncio.open_connection(
                "127.0.0.1", server.sockets[0].getsockname()[1]
            )
            peer = Peer(StreamChannel(reader, writer))
            await peer.send(Kind.REQUEST, b"git-upload-pack")
            assert (await peer.receive())[0] == Kind.READY
            writer.close()
            await writer.wait_closed()
            await asyncio.wait_for(finished.wait(), 5)
            assert not errors

    asyncio.run(run())


def test_invalid_service_is_rejected(tmp_path):
    class FakeChannel:
        def __init__(self):
            self.input = HEADER.pack(Kind.REQUEST, 2) + b"sh"
            self.sent = b""

        async def receive_exactly(self, count):
            data, self.input = self.input[:count], self.input[count:]
            return data

        async def send(self, data):
            self.sent += data

    channel = FakeChannel()
    asyncio.run(serve_git(channel, tmp_path))
    assert channel.sent[0] == Kind.ERROR


def test_oversized_frame_is_rejected_before_payload():
    class FakeChannel:
        async def receive_exactly(self, count):
            assert count == HEADER.size
            return HEADER.pack(Kind.DATA, MAX_FRAME + 1)

    with pytest.raises(ValueError, match="frame too large"):
        asyncio.run(Peer(FakeChannel()).receive())
