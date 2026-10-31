"""Keep the platform-specific Bluetooth implementation in aio-rfcomm."""

import asyncio
from contextlib import AsyncExitStack, asynccontextmanager
from uuid import UUID

import aio_rfcomm
from aio_rfcomm.macos import prompt_under_name

SERVICE = UUID("2d508ec7-69a4-47f2-a90c-b54df08c3cf5")


@asynccontextmanager
async def open_bluetooth():
    prompt_under_name(name="blugit", reason="Exchange Git repositories over Bluetooth")
    async with aio_rfcomm.open_bluetooth() as bluetooth:
        async with asyncio.timeout(60):
            async for state in bluetooth.watch_availability():
                if state == aio_rfcomm.Availability.AWAITING_PERMISSION:
                    continue
                if state != aio_rfcomm.Availability.USABLE:
                    raise RuntimeError(
                        f"Bluetooth unavailable: {state.name}. "
                        "Check the adapter, Bluetooth switch and OS permissions."
                    )
                break
        yield bluetooth


@asynccontextmanager
async def connect(address: str):
    async with open_bluetooth() as bluetooth:
        device = aio_rfcomm.RfcommDeviceInfo(address)
        # Bound discovery/connection, but allow long transfers on slow Bluetooth links.
        async with AsyncExitStack() as stack:
            async with asyncio.timeout(30):
                channel = await stack.enter_async_context(bluetooth.open_service(device, SERVICE))
            yield channel
