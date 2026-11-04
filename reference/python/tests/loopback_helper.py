"""Test-only transport substitution; installed blugit always uses Bluetooth."""

import asyncio
import os
from contextlib import asynccontextmanager

from blugit import helper


class StreamChannel:
    def __init__(self, reader, writer):
        self.reader = reader
        self.writer = writer

    async def send(self, data):
        # Exercise a stream where a frame header can arrive in multiple pieces.
        self.writer.write(data[:2])
        await self.writer.drain()
        await asyncio.sleep(0)
        self.writer.write(data[2:])
        await self.writer.drain()

    async def receive_exactly(self, size):
        return await self.reader.readexactly(size)


@asynccontextmanager
async def connect(_address, _channel=None):
    reader, writer = await asyncio.open_connection("127.0.0.1", int(os.environ["BLUGIT_TEST_PORT"]))
    try:
        yield StreamChannel(reader, writer)
    finally:
        writer.close()
        await writer.wait_closed()


if __name__ == "__main__":
    helper.connect = connect
    raise SystemExit(helper.main())
