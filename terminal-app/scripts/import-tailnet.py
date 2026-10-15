#!/usr/bin/env python3
"""Import public machine metadata from `tailscale status --json` for a device build.

No API credentials or Tailscale private state are copied. The iPhone continues
using its existing Tailscale app; a paired SSH host refreshes this cache later.
"""
import argparse
import getpass
import json
from pathlib import Path
import time

parser = argparse.ArgumentParser()
parser.add_argument("status", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--username", default=getpass.getuser())
args = parser.parse_args()
status = json.loads(args.status.read_text())
own = status["Self"]
machines = []
for node in [own, *status.get("Peer", {}).values()]:
    if node.get("OS", "").lower() in ("ios", "android", "tvos"):
        continue
    ips = node.get("TailscaleIPs", [])
    address = next((ip for ip in ips if ":" not in ip), next(iter(ips), ""))
    if not address:
        continue
    hostname = node.get("DNSName", "").rstrip(".")
    name = node.get("HostName", "")
    if name in ("", "localhost"):
        name = hostname.split(".")[0]
    machines.append(dict(name=name, hostname=hostname, address=address,
                         os=node.get("OS", ""), online=node.get("Online", False)))
source_ip = next(ip for ip in own["TailscaleIPs"] if ":" not in ip)
args.output.write_text(json.dumps(dict(machines=machines, source=f"{args.username}@{source_ip}",
                                      username=args.username, updated_at=int(time.time())), indent=2))
print(f"Imported {len(machines)} computers; no credentials included.")
