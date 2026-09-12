"""HTTP smoke for the isolated API in compose.demo.yaml; never reads .env."""
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


class SmokeError(Exception):
    """A safe, local error message without credentials or response bodies."""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


class Api:
    def __init__(self):
        self.base = os.environ.get("SYSGUD_DEMO_API_URL", "http://api:3000").rstrip("/")
        parsed = urllib.parse.urlsplit(self.base)
        if (parsed.scheme != "http" or parsed.hostname not in {"api", "localhost", "127.0.0.1"}
                or parsed.username or parsed.password or parsed.path or parsed.query or parsed.fragment):
            raise SmokeError("Demo URL must identify the isolated local API.")
        self.token = os.environ.get("SYSGUD_DEMO_API_TOKEN", "")
        if not self.token or "\n" in self.token or "\r" in self.token:
            raise SmokeError("Missing demo API token.")
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def call(self, path, expected=200, method="GET", data=None, authenticated=True):
        headers = {"Content-Type": "application/json"}
        if authenticated:
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(
            self.base + path, method=method, headers=headers,
            data=None if data is None else json.dumps(data).encode(),
        )
        try:
            try:
                response = self.opener.open(request, timeout=8)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                if response.status != expected:
                    raise SmokeError(f"Expected HTTP {expected}; received {response.status}.")
                raw = response.read(1024 * 1024 + 1)
                if len(raw) > 1024 * 1024:
                    raise SmokeError("API response exceeded the read limit.")
                return json.loads(raw)
        except (OSError, ValueError):
            raise SmokeError("Could not read a valid response from the isolated API.") from None


def require(condition, message):
    if not condition:
        raise SmokeError(message)


def run():
    api = Api()
    print("SYSGUD | Isolated container demo; no Telegram, model or supervised process", flush=True)
    require(api.call("/health").get("status") == "ok", "API is not healthy.")
    api.call("/api/v1/incidents", 401, authenticated=False)
    require(api.call("/api/v1/openapi.json").get("openapi") == "3.1.0", "OpenAPI version mismatch.")
    print("OK health, Bearer authentication, OpenAPI", flush=True)

    source = "container-demo-" + uuid.uuid4().hex
    api.call("/api/v1/events", 422, "POST", {
        "source": source, "message": "ERROR demo", "command": "untrusted",
    })
    marker = "redaction-check-" + uuid.uuid4().hex
    item = api.call("/api/v1/events", 201, "POST", {
        "source": source, "message": f"ERROR: explicit container demonstration token={marker}",
    })
    require(marker not in json.dumps(item), "Demo credential marker was not redacted.")
    require(item.get("source") == source, "API returned an unrelated incident.")
    require(item.get("status") == "pending_approval", "New incident is not pending approval.")
    require(item.get("proposed_action", {}).get("action_type") == "NOTIFY", "Demo requires the local NOTIFY fallback.")
    identifier = str(uuid.UUID(item["id"]))
    path = f"/api/v1/incidents/{identifier}"
    print(f"OK validation and redaction; own demo incident {identifier}", flush=True)

    # Only the new incident created above can receive a decision from this script.
    api.call(path + "/approve", 403, "POST", {"actor_id": 999})
    api.call(path + "/approve", 202, "POST", {"actor_id": 123})
    deadline = time.monotonic() + 10
    while True:
        item = api.call(path)
        require(item.get("id") == identifier and item.get("source") == source,
                "API returned an unrelated incident while waiting.")
        if item.get("status") == "executed":
            break
        require(item.get("status") in {"pending_approval", "executing"}, "Demo action failed.")
        require(time.monotonic() < deadline, "Demo action did not complete within 10 seconds.")
        time.sleep(0.1)
    require(item.get("decided_by") == 123, "Unexpected decision actor.")
    repeated = api.call(path + "/approve", 200, "POST", {"actor_id": 123})
    require(repeated == item, "Repeated approval changed the finished incident.")
    api.call(path + "/reject", 409, "POST", {"actor_id": 123})
    api.call("/api/v1/incidents?limit=0", 400)
    print("OK actor restriction, approval, NOTIFY completion, idempotency, conflicting decision, pagination", flush=True)
    print("CONTAINER DEMO PASSED. Persistence across restart is not tested by this disposable run.", flush=True)


if __name__ == "__main__":
    try:
        run()
    except SmokeError as error:
        print(f"Container demo failed: {error}", file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("Container demo failed: unexpected local error; no credentials or response bodies shown.", file=sys.stderr)
        sys.exit(1)
