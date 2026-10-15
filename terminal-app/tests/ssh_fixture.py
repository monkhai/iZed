"""Isolated SSH smoke fixture. Never executes commands or reads user credentials.

Usage: python ssh_fixture.py --bind IP --key device-public-key.pub --events events.jsonl
Requires paramiko. Accepts only that phone's public key. Stop with Ctrl-C.
"""
import argparse
import base64
import json
import io
import socket
import threading
import time
from pathlib import Path
import paramiko
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

parser = argparse.ArgumentParser()
parser.add_argument("--bind", default="127.0.0.1")
parser.add_argument("--port", type=int, default=22222)
parser.add_argument("--key", required=True)
parser.add_argument("--events", required=True)
args = parser.parse_args()
authorized = Path(args.key).read_text().split()[1]
host_key = paramiko.Ed25519Key.from_private_key(io.StringIO(
    Ed25519PrivateKey.generate().private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.OpenSSH,
        serialization.NoEncryption(),
    ).decode()
))
lock = threading.Lock()

def record(kind, **fields):
    with lock:
        with open(args.events, "a") as output:
            output.write(json.dumps(dict(kind=kind, **fields)) + "\n")

class Server(paramiko.ServerInterface):
    def __init__(self):
        self.shell = threading.Event()

    def get_allowed_auths(self, username):
        return "publickey"

    def check_auth_publickey(self, username, key):
        valid = key.get_base64() == authorized
        record("auth", accepted=valid)
        return paramiko.AUTH_SUCCESSFUL if valid else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, *rest):
        record("pty", term=term.decode(), columns=width, rows=height)
        return True

    def check_channel_window_change_request(self, channel, width, height, *rest):
        record("resize", columns=width, rows=height)
        return True

    def check_channel_shell_request(self, channel):
        self.shell.set()
        return True

def serve(connection):
    transport = paramiko.Transport(connection)
    try:
        transport.add_server_key(host_key)
        server = Server()
        transport.start_server(server=server)
        channel = transport.accept(15)
        if channel is None or not server.shell.wait(10):
            return
        # Split ANSI and multibyte UTF-8 deliberately across packets.
        for part in [b"\x1b[", b"32mSSH ready", b"\x1b[0m\r\nUTF-8: \xe2", b"\x9c\x93\r\n", b"smoke@host ~\r\n$ "]:
            channel.sendall(part)
            time.sleep(0.05)
        pending = bytearray()
        while True:
            data = channel.recv(4096)
            if not data:
                break
            for byte in data:
                if byte == 3:
                    record("control_c")
                    pending.clear()
                    channel.sendall(b"^C\r\n$ ")
                elif byte in (10, 13):
                    record("command", text=pending.decode(errors="replace"))
                    pending.clear()
                    channel.sendall(b"\r\nPHONE_SSH_OK\r\n$ ")
                elif byte in (8, 127):
                    pending[:] = pending[:-1]
                    channel.sendall(b"\b \b")
                else:
                    pending.append(byte)
                    channel.sendall(bytes([byte]))
    except (EOFError, OSError, paramiko.SSHException):
        pass  # Identity probing intentionally stops before authentication.
    finally:
        transport.close()

with socket.socket() as listener:
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((args.bind, args.port))
    listener.listen(8)
    print(f"SSH fixture listening on {args.bind}:{args.port}", flush=True)
    while True:
        connection, _ = listener.accept()
        threading.Thread(target=serve, args=(connection,), daemon=True).start()
