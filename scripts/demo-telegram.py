"""Crea un incidente de demo y observa la decision que tomes desde Telegram."""
import argparse
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
MAX_RESPONSE = 1024 * 1024
STATUSES = {"pending_approval", "executing", "executed", "failed", "rejected"}


class DemoError(Exception):
    """Mensajes locales que se pueden mostrar sin credenciales ni respuestas crudas."""


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        # Nunca reenviar el Bearer local a otro servidor.
        return None


def settings():
    try:
        lines = (ROOT / ".env").read_text(encoding="utf-8-sig").splitlines()
    except (OSError, UnicodeError):
        raise DemoError("No se pudo leer .env en la carpeta del proyecto.") from None
    result = {}
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        result[name.strip()] = value.strip().strip('"').strip("'")
    return result


class Api:
    def __init__(self, config):
        if config.get("SYSGUD_TELEGRAM_ENABLED", "") != "true":
            raise DemoError("Telegram no esta habilitado. Completa la vinculacion primero.")
        chat = config.get("TELEGRAM_CHAT_ID", "")
        if not chat.isascii() or not chat.isdigit() or int(chat) <= 0:
            raise DemoError("Falta TELEGRAM_CHAT_ID de una cuenta privada vinculada.")
        self.token = config.get("SYSGUD_BOT_API_TOKEN", "")
        if not self.token or "\r" in self.token or "\n" in self.token:
            raise DemoError("Falta un SYSGUD_BOT_API_TOKEN valido en .env.")
        try:
            port = int(config.get("SYSGUD_API_PORT", "3000"))
        except ValueError:
            raise DemoError("SYSGUD_API_PORT debe ser un numero entre 1 y 65535.") from None
        if not 1 <= port <= 65535:
            raise DemoError("SYSGUD_API_PORT debe estar entre 1 y 65535.")
        self.base = f"http://127.0.0.1:{port}"
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def call(self, path, *, method="GET", data=None, expected=200, auth=True, timeout=8):
        headers = {"Content-Type": "application/json"}
        if auth:
            headers["Authorization"] = f"Bearer {self.token}"
        try:
            request = urllib.request.Request(
                self.base + path, method=method, headers=headers,
                data=None if data is None else json.dumps(data).encode("utf-8"),
            )
            with self.opener.open(request, timeout=timeout) as response:
                if response.status != expected:
                    raise DemoError(f"Respuesta HTTP {response.status}; no se completo el paso.")
                raw = response.read(MAX_RESPONSE + 1)
                if len(raw) > MAX_RESPONSE:
                    raise DemoError("La respuesta de la API supero el limite de lectura.")
                return json.loads(raw)
        except urllib.error.HTTPError as error:
            raise DemoError(f"La API devolvio HTTP {error.code}. Revisa servidor y configuracion.") from None
        except (OSError, ValueError):
            raise DemoError("No se pudo consultar la API local o su respuesta no es valida.") from None


def incident_state(item, expected_id):
    if not isinstance(item, dict) or item.get("id") != expected_id:
        raise DemoError("La API no devolvio el incidente solicitado.")
    status = item.get("status")
    if status not in STATUSES:
        raise DemoError("La API devolvio un estado de incidente desconocido.")
    return status


def run(config, wait_seconds):
    api = Api(config)
    print("SYSGUD | Demo real con Telegram", flush=True)
    health = api.call("/health", auth=False)
    if not isinstance(health, dict) or health.get("status") != "ok":
        raise DemoError("La API no confirmo que el servicio este saludable.")
    api.call("/api/v1/incidents?limit=1")
    print("API saludable; autenticacion aceptada. Credenciales ocultas.", flush=True)
    item = api.call("/api/v1/events", method="POST", expected=201, timeout=75, data={
        "source": "hackathon-video",
        "message": "ERROR: incidente de DEMOSTRACION para el video de la hackathon; fallo simulado del servicio.",
    })
    try:
        incident_id = str(uuid.UUID(item["id"]))
    except (TypeError, KeyError, ValueError, AttributeError):
        raise DemoError("El evento fue enviado pero la API no devolvio un ID valido. Revisa /incidents en Telegram antes de repetir.") from None
    status = incident_state(item, incident_id)
    print(f"\nIncidente de demostracion: {incident_id}")
    action = item.get("proposed_action", {}).get("action_type")
    if action in {"NOTIFY", "EXECUTE", "KILL", "NONE"}:
        print(f"Accion propuesta: {action}")
    print("Abre el bot en Telegram y revisa la propuesta antes de decidir.")
    print(f"Aprobar:  /approve {incident_id}")
    print(f"Rechazar: /reject {incident_id}")
    print(f"Esperando tu decision hasta {wait_seconds} segundos...", flush=True)
    deadline = time.monotonic() + wait_seconds
    last_status = None
    while True:
        if status != last_status:
            print(f"Estado registrado: {status}", flush=True)
            last_status = status
        if status in {"executed", "rejected", "failed"}:
            actor = item.get("decided_by")
            if type(actor) is int and actor > 0:
                print(f"Decision registrada por el usuario: {actor}")
            if status == "executed":
                print("DEMO COMPLETADA: aprobacion registrada y accion completada.")
                return 0
            if status == "rejected":
                print("DEMO COMPLETADA: rechazo registrado; accion cancelada.")
                return 0
            print("DEMO INCOMPLETA: la accion termino en failed. Revisa el incidente en Telegram.")
            return 1
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            print(f"DEMO PENDIENTE: ultimo estado observado {status}. El incidente se conserva.")
            print(f"Puedes consultarlo en Telegram: /incident {incident_id}")
            return 2
        time.sleep(min(2, remaining))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            continue
        item = api.call(f"/api/v1/incidents/{incident_id}", timeout=min(8, remaining))
        status = incident_state(item, incident_id)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=int, default=120,
                        help="Segundos para esperar la decision (1..600; predeterminado: 120)")
    args = parser.parse_args()
    if not 1 <= args.timeout <= 600:
        parser.error("--timeout debe estar entre 1 y 600 segundos")
    try:
        return run(settings(), args.timeout)
    except KeyboardInterrupt:
        print("\nDemo interrumpida. El servidor y cualquier incidente creado se conservan.")
        return 130
    except DemoError as error:
        print(f"Demo incompleta: {error}", file=sys.stderr)
        return 1
    except Exception:
        # No mostrar excepciones arbitrarias: pueden contener cabeceras o cuerpos.
        print("Demo incompleta por un error local inesperado. Revisa la configuracion.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
