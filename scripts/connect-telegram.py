"""Pair a private Telegram account with an expiring link, without printing secrets."""
import argparse
import json
import pathlib
import re
import secrets
import sys
import time
import urllib.error
import urllib.request
import webbrowser

ROOT = pathlib.Path(__file__).resolve().parents[1]
ENV = ROOT / ".env"
PAIRING = ROOT / ".data" / "telegram-pairing.json"


def settings():
    result = {}
    for line in ENV.read_text(encoding="utf-8-sig").splitlines():
        if not line.strip() or line.lstrip().startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        result[name.strip()] = value.strip().strip('"').strip("'")
    return result


def telegram(token, method, body):
    request = urllib.request.Request(
        f"https://api.telegram.org/bot{token}/{method}",
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json"},
    )
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    try:
        with urllib.request.build_opener(NoRedirect).open(request, timeout=30) as response:
            data = response.read(1024 * 1024 + 1)
            if len(data) > 1024 * 1024:
                raise RuntimeError("Respuesta de Telegram demasiado grande")
            result = json.loads(data)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"Telegram HTTP {error.code}; revisa credencial o si existe otro polling/webhook") from None
    except (OSError, ValueError):
        raise RuntimeError("No se pudo consultar Telegram") from None
    if not result.get("ok"):
        raise RuntimeError("Telegram no confirmó la operación")
    return result["result"]


def update_settings(changes):
    text = ENV.read_text(encoding="utf-8-sig")
    for name, value in changes.items():
        pattern = re.compile(r"^[ \t]*" + re.escape(name) + r"[ \t]*=.*$", re.MULTILINE)
        entry = f"{name}={value}"
        if pattern.search(text):
            text = pattern.sub(lambda _: entry, text)
        else:
            text = text.rstrip() + "\n" + entry + "\n"
    temporary = ENV.with_name(".env.telegram.tmp")
    temporary.write_text(text, encoding="utf-8")
    temporary.replace(ENV)


def allowed_users(config):
    lists = []
    for name in ("SYSGUD_ALLOWED_TELEGRAM_USER_IDS", "TELEGRAM_ALLOWLIST"):
        users = set()
        for value in config.get(name, "").split(","):
            if value.strip():
                if not value.strip().isascii() or not value.strip().isdigit() or int(value.strip()) <= 0:
                    raise RuntimeError("La lista existente tiene un ID inválido")
                users.add(str(int(value.strip())))
        lists.append(users)
    if all(lists) and lists[0] != lists[1]:
        raise RuntimeError("Las dos listas existentes deben coincidir antes de vincular")
    return lists[0] or lists[1]


def poll(token, pairing):
    expected = "/start sysgud_" + pairing["code"]
    offset = pairing.get("offset", 0)
    updates = telegram(token, "getUpdates", {"offset": offset, "timeout": 20, "limit": 100, "allowed_updates": ["message"]})
    for update in updates:
        offset = max(offset, update["update_id"] + 1)
        message = update.get("message", {})
        sender = message.get("from", {})
        chat = message.get("chat", {})
        if (message.get("text") != expected or chat.get("type") != "private"
                or sender.get("is_bot", True) or sender.get("id") != chat.get("id")
                or type(sender.get("id")) is not int or sender["id"] <= 0
                or message.get("date", 0) < pairing["created_at"] - 5
                or message.get("date", 0) > pairing["created_at"] + 900):
            continue
        actor = str(sender["id"])
        allowed = allowed_users(settings())
        allowed.add(actor)
        users = ",".join(sorted(allowed, key=int))
        update_settings({"SYSGUD_ALLOWED_TELEGRAM_USER_IDS": users,
                         "TELEGRAM_ALLOWLIST": users,
                         "TELEGRAM_CHAT_ID": actor,
                         "SYSGUD_TELEGRAM_ENABLED": "true",
                         "SYSGUD_MONITOR_ENABLED": "false"})
        pairing.update({"paired": True, "actor_id": sender["id"], "offset": offset})
        PAIRING.write_text(json.dumps(pairing, indent=2), encoding="utf-8")
        # Acknowledge only through the pairing message; keep subsequent commands.
        telegram(token, "getUpdates", {"offset": offset, "timeout": 0, "limit": 1, "allowed_updates": ["message"]})
        return True
    pairing["offset"] = offset
    PAIRING.write_text(json.dumps(pairing, indent=2), encoding="utf-8")
    return False


def finish(token, pairing):
    commands = [("start", "Ayuda de Sysgud"), ("status", "Ver incidentes pendientes"),
                ("incidents", "Listar incidentes pendientes"), ("incident", "Ver detalle: /incident ID"),
                ("approve", "Aprobar: /approve ID"), ("reject", "Rechazar: /reject ID"),
                ("help", "Ver comandos disponibles")]
    telegram(token, "setMyCommands", {"commands": [{"command": c, "description": d} for c, d in commands]})
    print(json.dumps({"paired": True, "bot_username": pairing["bot_username"],
                      "actor_id": pairing["actor_id"], "next": "Ejecuta scripts/start.ps1"}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--begin", action="store_true", help="Crear o mostrar el enlace sin esperar")
    parser.add_argument("--open", action="store_true", help="Abrir el enlace en Telegram")
    parser.add_argument("--new-link", action="store_true", help="Invalidar el enlace local anterior")
    parser.add_argument("--wait", type=int, default=0, metavar="SECONDS", help="Esperar hasta 900 segundos")
    args = parser.parse_args()
    if not 0 <= args.wait <= 900:
        parser.error("--wait debe estar entre 0 y 900")
    config = settings()
    token = config.get("TELEGRAM_BOT_TOKEN", "")
    if not re.fullmatch(r"\d{5,16}:[A-Za-z0-9_-]{20,}", token):
        raise RuntimeError("Falta un token válido en .env")
    allowed = allowed_users(config)
    me = telegram(token, "getMe", {})
    if not re.fullmatch(r"[A-Za-z0-9_]+", me.get("username", "")) or not me.get("is_bot"):
        raise RuntimeError("Telegram no confirmó la identidad del bot")
    pairing = json.loads(PAIRING.read_text(encoding="utf-8-sig")) if PAIRING.exists() else {}
    same_bot = pairing.get("bot_username") == me["username"] and pairing.get("bot_id", me["id"]) == me["id"]
    if pairing.get("paired") and same_bot and not args.new_link:
        actor = str(pairing["actor_id"])
        if (actor not in allowed or config.get("TELEGRAM_CHAT_ID") != actor
                or config.get("SYSGUD_TELEGRAM_ENABLED") != "true"):
            raise RuntimeError("La configuración cambió; usa --new-link para vincular de nuevo")
        finish(token, pairing)
        return 0
    if telegram(token, "getWebhookInfo", {}).get("url"):
        raise RuntimeError("El bot tiene un webhook; no se modificará una integración existente")
    if args.new_link or not same_bot or time.time() - pairing.get("created_at", 0) >= 900:
        pairing = {"code": secrets.token_hex(16), "created_at": int(time.time()),
                   "bot_username": me["username"], "bot_id": me["id"], "offset": 0}
        PAIRING.parent.mkdir(parents=True, exist_ok=True)
        PAIRING.write_text(json.dumps(pairing, indent=2), encoding="utf-8")
    link = f"https://t.me/{me['username']}?start=sysgud_{pairing['code']}"
    print(f"Pulsa Iniciar / Start en {link}", flush=True)
    if args.open:
        webbrowser.open(link)
    if args.begin:
        return 0
    deadline = min(time.time() + args.wait, pairing["created_at"] + 900)
    while True:
        if poll(token, pairing):
            finish(token, pairing)
            return 0
        if time.time() >= deadline:
            print(json.dumps({"paired": False, "waiting_for": "Pulsa Iniciar en el enlace; vuelve a ejecutar este script"}), flush=True)
            return 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        # Whitelist our messages; arbitrary exception text can contain a token URL.
        print(str(error) if isinstance(error, RuntimeError) else "No se pudo completar la vinculación; revisa la configuración local", file=sys.stderr)
        sys.exit(1)
