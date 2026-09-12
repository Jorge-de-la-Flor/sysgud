"""Demo HTTP guiada; usa el mismo aislamiento que smoke-api.py, sin servicios externos."""
import argparse
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target"
BINARY = TARGET / "debug" / ("sysgud.exe" if os.name == "nt" else "sysgud")


class Demo:
    def __init__(self, automatic):
        self.automatic = automatic
        self.token = secrets.token_hex(32)
        self.process = None
        self.directory = None
        self.step = 0
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            self.port = probe.getsockname()[1]
        self.base = f"http://127.0.0.1:{self.port}"

    def call(self, path, method="GET", data=None, authenticated=True):
        headers = {"Content-Type": "application/json"}
        if authenticated:
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(
            self.base + path, headers=headers, method=method,
            data=None if data is None else json.dumps(data).encode(),
        )
        try:
            response = self.opener.open(request, timeout=8)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, json.load(response)

    def start(self):
        env = dict(
            os.environ, SYSGUD_LOAD_DOTENV="false", SYSGUD_BOT_API_TOKEN=self.token,
            SYSGUD_API_PORT=str(self.port), SYSGUD_DATABASE=str(self.directory / "state.sqlite"),
            SYSGUD_ALLOWED_TELEGRAM_USER_IDS="123", TELEGRAM_ALLOWLIST="",
            SYSGUD_MONITOR_ENABLED="false", SYSGUD_TELEGRAM_ENABLED="false",
            TELEGRAM_CHAT_ID="", TELEGRAM_BOT_TOKEN="", ANTHROPIC_API_KEY="",
            SYSGUD_COMMANDS_JSON="{}", SYSGUD_CONTEXT_LINES="12",
            SYSGUD_MAX_INCIDENTS="1000", SYSGUD_APPROVAL_TTL_SECONDS="86400",
            SYSGUD_TARGET_CMD=sys.executable, SYSGUD_TARGET_ARGS="[]",
        )
        self.process = subprocess.Popen(
            [str(BINARY)], cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError("El servidor no pudo iniciar; verifica el binario compilado.")
            try:
                if self.call("/health", authenticated=False)[0] == 200:
                    return
            except (OSError, ValueError):
                pass
            time.sleep(0.05)
        raise RuntimeError("El servidor no respondio en 15 segundos.")

    def stop(self):
        if self.process is not None:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=5)
            self.process = None

    def title(self, text):
        if self.step and not self.automatic:
            input("\nEnter para continuar...")
        self.step += 1
        print(f"\n[{self.step}/8] {text}", flush=True)

    def show(self, path, expected, method="GET", data=None, authenticated=True):
        print(f"{method} {path}")
        if data is not None:
            print(json.dumps(data, ensure_ascii=True))
        status, body = self.call(path, method, data, authenticated)
        print(f"HTTP {status}")
        # Solo datos creados por esta demo; nunca cabeceras ni credenciales.
        if isinstance(body, dict) and "id" in body:
            summary = {key: body[key] for key in ("id", "status", "decided_by", "decided_at")}
            summary["action"] = body["proposed_action"]["action_type"]
        else:
            summary = body
        print(json.dumps(summary, indent=2, ensure_ascii=True), flush=True)
        if status != expected:
            raise RuntimeError(f"Se esperaba HTTP {expected}; recibido {status}.")
        return body

    def run(self):
        if not BINARY.is_file():
            raise RuntimeError("Falta target/debug/sysgud: ejecuta cargo build --locked primero.")
        self.directory = Path(tempfile.mkdtemp(prefix="sysgud-demo-", dir=TARGET))
        try:
            self.start()
            print("SYSGUD | Demo local de la API", flush=True)
            print(f"Servidor: {self.base}")
            print("Token efimero oculto. SQLite temporal. .env no se carga ni modifica.")
            print("Sin modelo, Telegram ni procesos supervisados; accion local NOTIFY.")

            self.title("Salud del servicio")
            self.show("/health", 200, authenticated=False)

            self.title("Una solicitud sin token queda bloqueada")
            self.show("/api/v1/incidents", 401, authenticated=False)

            self.title("Crear un incidente y dejarlo pendiente de aprobacion")
            item = self.show("/api/v1/events", 201, "POST", {
                "source": "video-demo", "message": "ERROR: fallo de demostracion en el servicio",
            })
            if item["status"] != "pending_approval" or item["proposed_action"]["action_type"] != "NOTIFY":
                raise RuntimeError("La demo requiere un incidente pendiente con accion NOTIFY.")
            path = f"/api/v1/incidents/{item['id']}"

            self.title("Actor no autorizado: no puede aprobar")
            self.show(path + "/approve", 403, "POST", {"actor_id": 999})

            self.title("Aprobacion autorizada: se acepta el trabajo")
            self.show(path + "/approve", 202, "POST", {"actor_id": 123})

            self.title("Consultar el resultado hasta completar NOTIFY")
            deadline = time.monotonic() + 5
            while True:
                status, finished = self.call(path)
                if status != 200:
                    raise RuntimeError("No se pudo consultar el incidente.")
                if finished["status"] == "executed":
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError("El incidente no termino en executed.")
                time.sleep(0.05)
            self.show(path, 200)
            print("executed confirma NOTIFY; en esta demo no se ejecuta un comando del sistema.")

            self.title("Repetir la aprobacion devuelve el mismo resultado")
            repeated = self.show(path + "/approve", 200, "POST", {"actor_id": 123})
            if repeated != finished:
                raise RuntimeError("La repeticion cambio el incidente ya completado.")
            print("HTTP 200, mismo estado y decision: no se agenda otra ejecucion.")

            self.title("Reiniciar y comprobar persistencia")
            self.stop()
            self.start()
            recovered = self.show(path, 200)
            repeated = self.show(path + "/approve", 200, "POST", {"actor_id": 123})
            if recovered != finished or repeated != finished:
                raise RuntimeError("El estado no se conservo tras reiniciar.")
            print("\nDEMO OK: autenticacion, aprobacion, repeticion y persistencia verificadas.")
        finally:
            self.stop()
            # Borrar unicamente el directorio temporal propio, dentro de target.
            if self.directory is not None:
                resolved = self.directory.resolve()
                if (self.directory.is_symlink() or resolved.parent != TARGET.resolve()
                        or not resolved.name.startswith("sysgud-demo-")):
                    raise RuntimeError("Se cancelo la limpieza: ruta temporal fuera de target.")
                shutil.rmtree(resolved)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--auto", action="store_true", help="Ejecutar sin pausas entre pasos")
    args = parser.parse_args()
    try:
        Demo(args.auto).run()
    except (KeyboardInterrupt, EOFError):
        print("\nDemo interrumpida; servidor propio detenido y datos temporales eliminados.")
        return 130
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Demo incompleta: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
