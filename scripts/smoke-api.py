"""Exercise the real binary over HTTP, with isolated state and no external services."""
import argparse
import concurrent.futures
import json
import os
import pathlib
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=pathlib.Path,
                    default=ROOT / "target" / "debug" / ("sysgud.exe" if os.name == "nt" else "sysgud"),
                    help="Executable to verify; defaults to the debug build")
BINARY = parser.parse_args().binary.resolve()
if not BINARY.is_file():
    parser.error("The requested executable does not exist; compile it first")
TOKEN = secrets.token_hex(32)
with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    PORT = probe.getsockname()[1]
BASE = f"http://127.0.0.1:{PORT}"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def call(path, method="GET", data=None, authenticated=True):
    headers = {"Content-Type": "application/json"}
    if authenticated:
        headers["Authorization"] = f"Bearer {TOKEN}"
    request = urllib.request.Request(BASE + path, headers=headers, method=method,
                                     data=None if data is None else json.dumps(data).encode())
    try:
        response = opener.open(request, timeout=8)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, json.load(response)


def start(database):
    env = dict(os.environ, SYSGUD_LOAD_DOTENV="false", SYSGUD_BOT_API_TOKEN=TOKEN,
               SYSGUD_API_PORT=str(PORT), SYSGUD_API_HOST="127.0.0.1", SYSGUD_DATABASE=str(database),
               SYSGUD_ALLOWED_TELEGRAM_USER_IDS="123", TELEGRAM_ALLOWLIST="",
               SYSGUD_MONITOR_ENABLED="false", SYSGUD_TELEGRAM_ENABLED="false",
               TELEGRAM_CHAT_ID="", TELEGRAM_BOT_TOKEN="", ANTHROPIC_API_KEY="",
               SYSGUD_COMMANDS_JSON="{}", SYSGUD_CONTEXT_LINES="12",
               SYSGUD_MAX_INCIDENTS="1000", SYSGUD_APPROVAL_TTL_SECONDS="900")
    process = subprocess.Popen([str(BINARY)], cwd=ROOT, env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                               creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError("sysgud failed to start: " + process.stderr.read(8192).decode(errors="replace"))
            try:
                if call("/health", authenticated=False)[0] == 200:
                    return process
            except (OSError, ValueError):
                pass
            time.sleep(0.05)
        raise RuntimeError("sysgud did not become ready")
    except BaseException:
        stop(process)
        raise


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
    finally:
        process.stderr.close()


with tempfile.TemporaryDirectory(prefix="sysgud-http-", dir=ROOT / "target") as directory:
    database = pathlib.Path(directory) / "state.sqlite"
    process = start(database)
    try:
        assert call("/api/v1/incidents", authenticated=False)[0] == 401
        assert call("/api/v1/events", "POST", {"source":"smoke","message":"ERROR","command":"untrusted"})[0] == 422
        status, item = call("/api/v1/events", "POST", {"source":"smoke","message":"ERROR token=smoke-secret"})
        assert status == 201 and "smoke-secret" not in json.dumps(item)
        path = f"/api/v1/incidents/{item['id']}"
        assert call(path + "/approve", "POST", {"actor_id":999})[0] == 403
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(lambda _: call(path + "/approve", "POST", {"actor_id":123}), range(2)))
        assert sorted(status for status, _ in results) == [200, 202]
        deadline = time.monotonic() + 5
        while call(path)[1]["status"] != "executed":
            assert time.monotonic() < deadline
            time.sleep(0.01)
        assert call("/api/v1/incidents?limit=0")[0] == 400
        assert call("/api/v1/openapi.json")[1]["openapi"] == "3.1.0"
    finally:
        stop(process)
    process = start(database)
    try:
        status, item = call(path + "/approve", "POST", {"actor_id":123})
        assert status == 200 and item["status"] == "executed"
        assert call(path + "/reject", "POST", {"actor_id":123})[0] == 409
    finally:
        stop(process)
print("HTTP smoke: authentication, validation, redaction, concurrent approval, OpenAPI and restart persistence OK")
