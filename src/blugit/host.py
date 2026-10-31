import asyncio
import os
import sys
from pathlib import Path

from .bluetooth import SERVICE, open_bluetooth
from .wire import CHUNK, Kind, Peer

SERVICES = {b"git-upload-pack": "upload-pack", b"git-receive-pack": "receive-pack"}


def git_environment():
    # A host launched inside another Git process must still serve its configured repo.
    return {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


async def serve_git(channel, repository: Path):
    """Run exactly one Git service for the single, locally configured repository."""
    peer = Peer(channel)
    process = None
    tasks = []
    try:
        async with asyncio.timeout(15):
            kind, request = await peer.receive()
        if kind != Kind.REQUEST or request not in SERVICES:
            await peer.send(Kind.ERROR, b"expected git-upload-pack or git-receive-pack")
            return
        process = await asyncio.create_subprocess_exec(
            "git",
            SERVICES[request],
            str(repository),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=git_environment(),
        )
        await peer.send(Kind.READY)
        exited = asyncio.Event()

        async def incoming():
            eof = False
            while True:
                kind, data = await peer.receive()
                if kind == Kind.ACK and exited.is_set() and not data:
                    return
                if kind == Kind.EOF and not eof and not data:
                    eof = True
                    process.stdin.close()
                elif kind == Kind.DATA and not eof:
                    try:
                        process.stdin.write(data)
                        await process.stdin.drain()
                    except (BrokenPipeError, ConnectionResetError):
                        # Git can reject a request before the guest finishes sending.
                        eof = True
                else:
                    raise ValueError("unexpected guest frame")

        async def output(stream, kind):
            while data := await stream.read(CHUNK):
                await peer.send(kind, data)

        async def outgoing():
            async with asyncio.TaskGroup() as group:
                group.create_task(output(process.stdout, Kind.DATA))
                group.create_task(output(process.stderr, Kind.STDERR))
            code = await process.wait()
            exited.set()
            await peer.send(Kind.EXIT, str(code).encode("ascii"))

        tasks = [asyncio.create_task(incoming()), asyncio.create_task(outgoing())]
        # Acknowledging EXIT avoids closing RFCOMM with output still in flight.
        done, _ = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        for task in done:
            task.result()  # Disconnects must also stop a Git process waiting for stdin.
        async with asyncio.timeout(10):
            await asyncio.gather(*tasks)
    finally:
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        if process is not None and process.returncode is None:
            process.kill()
            await process.wait()


async def host(repository: Path):
    async def handle(request):
        try:
            async with request.accept() as channel:
                print(f"Connected: {request.info.address}", file=sys.stderr, flush=True)
                await serve_git(channel, repository)
        except Exception as exc:
            print(f"Connection ended: {exc}", file=sys.stderr, flush=True)

    async with open_bluetooth() as bluetooth:
        async with bluetooth.serve(SERVICE, name="blugit prototype") as service:
            print(f"Serving {repository} (RFCOMM channel {service.channel})", flush=True)
            print("On the guest: blugit devices", flush=True)
            async with asyncio.TaskGroup() as group:
                async for request in service.connection_requests():
                    group.create_task(handle(request))
