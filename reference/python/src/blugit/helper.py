import asyncio
import contextlib
import os
import re
import sys
import threading
from urllib.parse import parse_qs, urlsplit

from .bluetooth import connect
from .wire import CHUNK, Kind, Peer


def parse_url(url: str) -> tuple[str, int | None]:
    parsed = urlsplit(url)
    address = parsed.netloc.replace("-", ":")
    if (
        parsed.scheme != "blugit"
        or not re.fullmatch(r"(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", address)
        or parsed.path not in ("", "/")
        or parsed.fragment
    ):
        raise ValueError("expected blugit://AA-BB-CC-DD-EE-FF (one repo per host)")
    channel = None
    if parsed.query:
        query = parse_qs(parsed.query, keep_blank_values=True, strict_parsing=True)
        if set(query) != {"channel"} or len(query["channel"]) != 1:
            raise ValueError("only one optional ?channel=1..30 is supported")
        value = query["channel"][0]
        if not re.fullmatch(r"[0-9]{1,2}", value) or not 1 <= int(value) <= 30:
            raise ValueError("RFCOMM channel must be between 1 and 30")
        channel = int(value)
    return address.upper(), channel


def binary_stdio():
    if sys.platform == "win32":
        import msvcrt

        for stream in (sys.stdin, sys.stdout, sys.stderr):
            msvcrt.setmode(stream.fileno(), os.O_BINARY)


async def transfer(address: str, service: bytes, channel_number: int | None = None) -> int:
    async with connect(address, channel_number) as channel:
        peer = Peer(channel)
        await peer.send(Kind.REQUEST, service)
        async with asyncio.timeout(15):
            kind, data = await peer.receive()
        if kind == Kind.ERROR:
            raise RuntimeError(data.decode("utf-8", "replace"))
        if kind != Kind.READY or data:
            raise RuntimeError("invalid host response")
        sys.stdout.buffer.write(b"\n")  # Remote Helper connect succeeded.
        sys.stdout.buffer.flush()

        loop = asyncio.get_running_loop()
        input_done = loop.create_future()

        def finish_input(error):
            if not input_done.done():
                if error is None:
                    input_done.set_result(None)
                else:
                    input_done.set_exception(error)

        def read_input():
            error = None
            try:
                while data := os.read(sys.stdin.fileno(), CHUNK):
                    asyncio.run_coroutine_threadsafe(peer.send(Kind.DATA, data), loop).result()
                asyncio.run_coroutine_threadsafe(peer.send(Kind.EOF), loop).result()
            except Exception as exc:
                error = exc
            with contextlib.suppress(RuntimeError):
                loop.call_soon_threadsafe(finish_input, error)

        # Git may keep stdin open until the helper exits. A blocked daemon reader
        # must not hold up process exit (asyncio.to_thread would wait at shutdown).
        threading.Thread(target=read_input, daemon=True).start()

        async def receive_output():
            while True:
                kind, data = await peer.receive()
                if kind == Kind.DATA:
                    sys.stdout.buffer.write(data)
                    sys.stdout.buffer.flush()
                elif kind in (Kind.STDERR, Kind.ERROR):
                    sys.stderr.buffer.write(data)
                    sys.stderr.buffer.flush()
                    if kind == Kind.ERROR:
                        return 1
                elif kind == Kind.EXIT:
                    code = int(data)
                    await peer.send(Kind.ACK)
                    return 0 if code == 0 else 1
                else:
                    raise RuntimeError("unexpected host frame")

        receiver = asyncio.create_task(receive_output())
        try:
            done, _ = await asyncio.wait(
                [receiver, input_done], return_when=asyncio.FIRST_COMPLETED
            )
            if input_done in done:
                input_done.result()
            return await receiver
        finally:
            receiver.cancel()
            input_done.cancel()
            await asyncio.gather(receiver, return_exceptions=True)


def main():
    binary_stdio()
    try:
        if len(sys.argv) not in (2, 3):
            raise ValueError("git-remote-blugit is invoked by Git")
        address, channel = parse_url(sys.argv[-1])
        # Read control lines without buffering bytes needed by the later binary relay.
        while line := sys.stdin.buffer.raw.readline(1024):
            command = line.rstrip(b"\n")
            if command == b"capabilities":
                sys.stdout.buffer.write(b"connect\n\n")
                sys.stdout.buffer.flush()
            elif command in (b"connect git-upload-pack", b"connect git-receive-pack"):
                return asyncio.run(transfer(address, command.split(b" ", 1)[1], channel))
            elif not command:
                return 0
            else:
                raise ValueError(f"unsupported helper command: {command!r}")
        return 0
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        print(f"blugit: {exc}", file=sys.stderr)
        return 1
