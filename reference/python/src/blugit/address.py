"""Read this computer's Bluetooth address without enumerating paired devices."""

import asyncio
import json
import subprocess
import sys

from aio_rfcomm import RfcommDeviceInfo


def clone_url(address: str, channel: int | None = None) -> str:
    address = RfcommDeviceInfo(address).address
    if address == "00:00:00:00:00:00":
        raise ValueError("the adapter did not report a usable Bluetooth address")
    url = "blugit://" + address.replace(":", "-")
    if channel is not None:
        if not 1 <= channel <= 30:
            raise ValueError("RFCOMM channel must be between 1 and 30")
        url += f"?channel={channel}"
    return url


async def windows_address() -> str:
    from winrt.windows.devices.bluetooth import BluetoothAdapter

    adapter = await BluetoothAdapter.get_default_async()
    if adapter is None or not adapter.is_classic_supported:
        raise RuntimeError("No Bluetooth Classic adapter found")
    raw = f"{adapter.bluetooth_address:012X}"
    return ":".join(raw[index : index + 2] for index in range(0, 12, 2))


def macos_address() -> str:
    # Use the OS utility instead of loading IOBluetooth into the CLI process:
    # aio-rfcomm handles macOS Bluetooth permissions in its own helper process.
    result = subprocess.run(
        ["/usr/sbin/system_profiler", "SPBluetoothDataType", "-json"],
        capture_output=True,
        text=True,
        check=True,
        timeout=15,
    )
    for item in json.loads(result.stdout).get("SPBluetoothDataType", []):
        address = item.get("controller_properties", {}).get("controller_address")
        if address:
            return RfcommDeviceInfo(address).address
    raise RuntimeError("macOS did not report a Bluetooth controller address")


async def local_address() -> str:
    async with asyncio.timeout(20):
        if sys.platform == "win32":
            address = await windows_address()
        elif sys.platform == "darwin":
            address = await asyncio.to_thread(macos_address)
        else:
            raise RuntimeError("Automatic local address lookup supports Windows and macOS only")
    clone_url(address)  # Reject an unavailable/all-zero address before showing it.
    return RfcommDeviceInfo(address).address
