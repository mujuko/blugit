import argparse
import asyncio
import subprocess
import sys
from pathlib import Path

from .bluetooth import open_bluetooth
from .host import git_environment, host


async def devices():
    async with open_bluetooth() as bluetooth:
        known = await bluetooth.list_known_devices()
        for device in known:
            address = device.address.replace(":", "-")
            print(f"{device.name or '(unnamed)'}\t{device.address}\tblugit://{address}")
        if not known:
            print("No paired devices. Pair the computers in OS Bluetooth settings first.")


def main():
    parser = argparse.ArgumentParser(description="Minimal Git over Bluetooth RFCOMM")
    commands = parser.add_subparsers(dest="command", required=True)
    serve = commands.add_parser("host", help="publish one existing bare repository")
    serve.add_argument("repository", type=Path)
    commands.add_parser("devices", help="list OS-paired devices and their clone URLs")
    args = parser.parse_args()
    try:
        if args.command == "devices":
            asyncio.run(devices())
        else:
            repository = args.repository.resolve(strict=True)
            result = subprocess.run(
                ["git", "rev-parse", "--is-bare-repository"],
                cwd=repository,
                capture_output=True,
                text=True,
                env=git_environment(),
                check=True,
            )
            if result.stdout.strip() != "true":
                raise ValueError("host requires a bare repository: git init --bare <path>")
            asyncio.run(host(repository))
        return 0
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        print(f"blugit: {exc}", file=sys.stderr)
        return 1
