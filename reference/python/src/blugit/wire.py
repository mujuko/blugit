"""A tiny framing layer: RFCOMM itself has no messages or half-close."""

import asyncio
import struct
from enum import IntEnum
from typing import Protocol

CHUNK = 32768
MAX_FRAME = 65536
HEADER = struct.Struct("!BI")


class Channel(Protocol):
    async def send(self, data: bytes) -> None: ...
    async def receive_exactly(self, count: int) -> bytes: ...


class Kind(IntEnum):
    REQUEST = 1
    READY = 2
    DATA = 3
    EOF = 4
    STDERR = 5
    EXIT = 6
    ERROR = 7
    ACK = 8


class Peer:
    def __init__(self, channel: Channel):
        self.channel = channel
        self.lock = asyncio.Lock()

    async def send(self, kind: Kind, data: bytes = b"") -> None:
        if len(data) > MAX_FRAME:
            raise ValueError("frame too large")
        async with self.lock:
            await self.channel.send(HEADER.pack(kind, len(data)) + data)

    async def receive(self) -> tuple[Kind, bytes]:
        kind, size = HEADER.unpack(await self.channel.receive_exactly(HEADER.size))
        if size > MAX_FRAME:
            raise ValueError("frame too large")
        data = await self.channel.receive_exactly(size) if size else b""
        return Kind(kind), data
