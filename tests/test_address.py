import asyncio
import importlib
import json
from contextlib import asynccontextmanager
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from blugit import bluetooth, cli
from blugit.address import clone_url
from blugit.helper import parse_url

address_module = importlib.import_module("blugit.address")
host_module = importlib.import_module("blugit.host")


def test_address_command_reads_local_adapter_not_paired_devices(monkeypatch, capsys):
    monkeypatch.setattr(cli, "local_address", AsyncMock(return_value="00:A7:50:41:8D:47"))
    monkeypatch.setattr(cli, "open_bluetooth", lambda: pytest.fail("must not list remote devices"))
    monkeypatch.setattr("sys.argv", ["blugit", "address"])
    assert cli.main() == 0
    assert "blugit://00-A7-50-41-8D-47" in capsys.readouterr().out


def test_unpaired_host_prints_both_clone_urls(monkeypatch, capsys, tmp_path):
    async def requests():
        return
        yield  # Make an empty async iterator: no guest needs to be paired/connected.

    @asynccontextmanager
    async def serve(*args, **kwargs):
        yield SimpleNamespace(channel=7, connection_requests=requests)

    @asynccontextmanager
    async def open_bluetooth():
        # Deliberately no list_known_devices method.
        yield SimpleNamespace(serve=serve)

    monkeypatch.setattr(host_module, "open_bluetooth", open_bluetooth)
    monkeypatch.setattr(host_module, "local_address", AsyncMock(return_value="00:A7:50:41:8D:47"))
    asyncio.run(host_module.host(tmp_path))
    output = capsys.readouterr().out
    assert "clone blugit://00-A7-50-41-8D-47 demo-work" in output
    assert "blugit://00-A7-50-41-8D-47?channel=7" in output


def test_missing_paired_devices_points_to_host_url(monkeypatch, capsys):
    @asynccontextmanager
    async def open_bluetooth():
        yield SimpleNamespace(list_known_devices=AsyncMock(return_value=[]))

    monkeypatch.setattr(cli, "open_bluetooth", open_bluetooth)
    asyncio.run(cli.devices())
    output = capsys.readouterr().out
    assert "not nearby discovery" in output
    assert "Copy the clone URL" in output
    assert "Pair the computers" not in output


@pytest.mark.parametrize("channel", [1, 7, 30])
def test_direct_url_roundtrip(channel):
    assert parse_url(clone_url("00:a7:50:41:8d:47", channel)) == ("00:A7:50:41:8D:47", channel)


@pytest.mark.parametrize(
    "query",
    [
        "channel=0",
        "channel=31",
        "channel=-1",
        "channel=abc",
        "channel=",
        "channel=1&channel=2",
        "channel=1&other=2",
    ],
)
def test_invalid_direct_url(query):
    with pytest.raises(ValueError):
        parse_url("blugit://00-A7-50-41-8D-47?" + query)


def test_direct_channel_skips_service_discovery(monkeypatch):
    sentinel = object()

    @asynccontextmanager
    async def open_channel(device, channel):
        assert device.address == "00:A7:50:41:8D:47"
        assert channel == 7
        yield sentinel

    @asynccontextmanager
    async def open_bluetooth():
        # No open_service or list_known_devices: the direct route must not use them.
        yield SimpleNamespace(open_channel=open_channel)

    monkeypatch.setattr(bluetooth, "open_bluetooth", open_bluetooth)

    async def run():
        async with bluetooth.connect("00:A7:50:41:8D:47", 7) as channel:
            assert channel is sentinel

    asyncio.run(run())


def test_macos_uses_controller_address_instead_of_connected_devices(monkeypatch):
    report = {
        "SPBluetoothDataType": [
            {
                "controller_properties": {"controller_address": "00-A7-50-41-8D-47"},
                "device_connected": [{"headphones": {"device_address": "AA-BB-CC-DD-EE-FF"}}],
            }
        ],
    }
    monkeypatch.setattr(
        address_module.subprocess,
        "run",
        lambda *a, **kw: SimpleNamespace(stdout=json.dumps(report)),
    )
    assert address_module.macos_address() == "00:A7:50:41:8D:47"


def test_zero_address_does_not_produce_a_bogus_clone_url():
    with pytest.raises(ValueError, match="usable Bluetooth address"):
        clone_url("00:00:00:00:00:00")
