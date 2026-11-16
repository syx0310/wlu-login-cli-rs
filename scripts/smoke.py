#!/usr/bin/env python3
"""Exercise the built executable against a local SRUN fixture; never use real credentials."""

import argparse
import http.server
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import threading
import tomllib
from urllib.parse import parse_qs, urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--runner", default="")
    parser.add_argument("--interface")
    parser.add_argument("--http-only", action="store_true")
    args = parser.parse_args()
    seen = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlsplit(self.path)
            query = {key: value[0] for key, value in parse_qs(url.query).items()}
            seen.append((url.path, query))
            response = {"error": "ok", "res": "ok", "online_ip": "127.0.0.1", "user_name": "fixture"}
            if url.path == "/cgi-bin/get_challenge":
                response["challenge"] = "1234567890abcdef"
            data = (query.get("callback", "cb") + "(" + json.dumps(response) + ")").encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = {key: value for key, value in os.environ.items() if "proxy" not in key.lower()}
    base = shlex.split(args.runner) + [str(args.binary.resolve())]
    root = Path(__file__).resolve().parent.parent
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]

    def run(arguments, success=True):
        result = subprocess.run(base + arguments, env=env, capture_output=True, text=True, timeout=45)
        if (result.returncode == 0) != success:
            raise RuntimeError(f"command {arguments!r}: exit={result.returncode}\n{result.stdout}\n{result.stderr}")
        return result.stdout + result.stderr

    try:
        assert "Usage:" in run(["--help"])
        assert f"wlu-login-cli {version}" in run(["--version"])
        with tempfile.TemporaryDirectory() as directory:
            endpoint = f"http://127.0.0.1:{server.server_port}"
            common = ["--config", str(Path(directory) / "config.yaml"), "-e", endpoint, "-a", "1"]
            if args.interface:
                common += ["-f", args.interface]
            assert "fixture" in run(["net", "info"] + common)
            run(["net", "login", "-u", "test+user@example.edu", "-p", "test & password"] + common)
            login = next(query for path, query in seen if path == "/cgi-bin/srun_portal")
            assert login["username"] == "test+user@example.edu"
            assert login["password"].startswith("{MD5}")
            assert login["info"].startswith("{SRBX1}")
            assert len(login["chksum"]) == 40
            run(["net", "logout"] + common)
            assert any(path == "/cgi-bin/rad_user_dm" for path, _ in seen)
            if args.interface:
                run(["net", "info"] + common + ["-f", "wlu-missing0"], success=False)
            if args.http_only:
                run(["net", "info"] + common + ["-e", endpoint.replace("http:", "https:")], success=False)
        print(f"Smoke test passed: help, version, info, login, logout; interface={args.interface}, http_only={args.http_only}")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
