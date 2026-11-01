import argparse
import asyncio
import subprocess
import sys
from pathlib import Path

from .address import clone_url, local_address
from .bluetooth import open_bluetooth
from .host import git_environment, host


async def devices():
    print("Paired devices only; this is not nearby discovery.")
    async with open_bluetooth() as bluetooth:
        known = await bluetooth.list_known_devices()
        for device in known:
            address = device.address.replace(":", "-")
            print(f"{device.name or '(unnamed)'}\t{device.address}\tblugit://{address}")
        if not known:
            print("No paired devices. Copy the clone URL from 'blugit host' on the host PC.")
        print("If the host is missing, run 'blugit address' on it or copy its host startup URL.")


async def address():
    own_address = await local_address()
    print(f"Bluetooth address: {own_address}")
    print(f"URL: {clone_url(own_address)}")


def main():
    parser = argparse.ArgumentParser(description="Minimal Git over Bluetooth RFCOMM")
    commands = parser.add_subparsers(dest="command", required=True)
    serve = commands.add_parser("host", help="publish one existing bare repository")
    serve.add_argument("repository", type=Path)
    serve.add_argument("--address", help="override the local Bluetooth address shown to the guest")
    commands.add_parser("devices", help="list OS-paired devices and their clone URLs")
    commands.add_parser("address", help="show this PC's Bluetooth address and clone URL")
    args = parser.parse_args()
    try:
        if args.command == "devices":
            asyncio.run(devices())
        elif args.command == "address":
            asyncio.run(address())
        else:
            if args.address:
                clone_url(args.address)  # Validate before starting the service.
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
            asyncio.run(host(repository, args.address))
        return 0
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        print(f"blugit: {exc}", file=sys.stderr)
        return 1
