"""Offline checks for Telegram pairing; never read the real environment or network."""
import contextlib
import copy
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "connect_telegram", pathlib.Path(__file__).with_name("connect-telegram.py")
)
CONNECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONNECT)


class PairingTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="sysgud-pairing-test-")
        self.addCleanup(self.directory.cleanup)
        root = pathlib.Path(self.directory.name)
        self.env = root / ".env"
        self.pairing_path = root / "pairing.json"
        self.env.write_text(
            "# Preserve unrelated application settings\n"
            "TELEGRAM_BOT_TOKEN=12345:offline_test_placeholder_000\n"
            "OTHER_SETTING='keep this value'\n"
            " SYSGUD_ALLOWED_TELEGRAM_USER_IDS=17\n"
            "TELEGRAM_ALLOWLIST=17\n",
            encoding="utf-8",
        )
        self.pairing = {
            "code": "offline_nonce", "created_at": 1000,
            "bot_username": "offline_bot", "bot_id": 500, "offset": 10,
        }
        self.pairing_path.write_text(json.dumps(self.pairing), encoding="utf-8")
        self.env_patch = mock.patch.object(CONNECT, "ENV", self.env)
        self.path_patch = mock.patch.object(CONNECT, "PAIRING", self.pairing_path)
        self.env_patch.start()
        self.path_patch.start()
        self.addCleanup(self.env_patch.stop)
        self.addCleanup(self.path_patch.stop)
        # Prevent accidental network access if a test misses the Telegram mock.
        self.network_patch = mock.patch.object(
            CONNECT.urllib.request, "build_opener",
            side_effect=AssertionError("Network is forbidden in offline tests"),
        )
        self.network_patch.start()
        self.addCleanup(self.network_patch.stop)

    def message(self):
        return {
            "update_id": 12,
            "message": {
                "text": "/start sysgud_offline_nonce", "date": 1001,
                "from": {"id": 42, "is_bot": False},
                "chat": {"id": 42, "type": "private"},
            },
        }

    def test_valid_pair_preserves_settings_and_existing_users(self):
        with mock.patch.object(CONNECT, "telegram", side_effect=[[self.message()], []]) as api:
            self.assertTrue(CONNECT.poll("offline", self.pairing))
        settings = CONNECT.settings()
        self.assertEqual(settings["OTHER_SETTING"], "keep this value")
        self.assertEqual(settings["SYSGUD_ALLOWED_TELEGRAM_USER_IDS"], "17,42")
        self.assertEqual(settings["TELEGRAM_ALLOWLIST"], "17,42")
        self.assertEqual(settings["TELEGRAM_CHAT_ID"], "42")
        self.assertEqual(settings["SYSGUD_TELEGRAM_ENABLED"], "true")
        self.assertEqual(settings["SYSGUD_MONITOR_ENABLED"], "false")
        self.assertIn("# Preserve unrelated application settings", self.env.read_text())
        self.assertNotIn(".env.telegram.tmp", [path.name for path in self.env.parent.iterdir()])
        saved = json.loads(self.pairing_path.read_text())
        self.assertEqual(saved["actor_id"], 42)
        self.assertTrue(saved["paired"])
        self.assertEqual(api.call_args_list[-1].args[2]["offset"], 13)

    def test_rejects_unauthorized_messages_without_changing_env(self):
        cases = {
            "plain_start": lambda message: message.update(text="/start"),
            "different_nonce": lambda message: message.update(text="/start sysgud_other"),
            "nonce_prefix": lambda message: message.update(text="/start sysgud_offline_nonce_extra"),
            "group": lambda message: message["chat"].update(type="group"),
            "bot_sender": lambda message: message["from"].update(is_bot=True),
            "missing_bot_flag": lambda message: message["from"].pop("is_bot"),
            "different_chat": lambda message: message["chat"].update(id=43),
            "zero_id": lambda message: (message["from"].update(id=0), message["chat"].update(id=0)),
            "negative_id": lambda message: (message["from"].update(id=-42), message["chat"].update(id=-42)),
            "string_id": lambda message: (message["from"].update(id="42"), message["chat"].update(id="42")),
            "boolean_id": lambda message: (message["from"].update(id=True), message["chat"].update(id=True)),
            "old_message": lambda message: message.update(date=994),
            "expired_message": lambda message: message.update(date=1901),
        }
        original = self.env.read_bytes()
        for name, mutate in cases.items():
            with self.subTest(name=name):
                update = self.message()
                mutate(update["message"])
                pairing = copy.deepcopy(self.pairing)
                with mock.patch.object(CONNECT, "telegram", return_value=[update]) as api:
                    self.assertFalse(CONNECT.poll("offline", pairing))
                self.assertEqual(api.call_count, 1)
                self.assertEqual(self.env.read_bytes(), original)
                self.assertNotIn("actor_id", pairing)
                self.assertEqual(pairing["offset"], 13)

    def test_conflicting_allowlist_is_not_silently_merged(self):
        self.env.write_text(
            "SYSGUD_ALLOWED_TELEGRAM_USER_IDS=17\nTELEGRAM_ALLOWLIST=18\n",
            encoding="utf-8",
        )
        original = self.env.read_bytes()
        with mock.patch.object(CONNECT, "telegram", return_value=[self.message()]) as api:
            with self.assertRaisesRegex(RuntimeError, "deben coincidir"):
                CONNECT.poll("offline", self.pairing)
        self.assertEqual(self.env.read_bytes(), original)
        self.assertEqual(api.call_count, 1)
        self.assertNotIn("actor_id", self.pairing)

    def test_invalid_existing_ids_are_rejected(self):
        for value in ("0", "-1", "not-an-id", "１２３", "12.0"):
            with self.subTest(value=value):
                with self.assertRaisesRegex(RuntimeError, "ID inválido"):
                    CONNECT.allowed_users({"TELEGRAM_ALLOWLIST": value})

    def test_acknowledges_only_through_pairing_message(self):
        next_message = self.message()
        next_message["update_id"] = 13
        next_message["message"]["text"] = "/status"
        with mock.patch.object(CONNECT, "telegram", side_effect=[[self.message(), next_message], []]) as api:
            self.assertTrue(CONNECT.poll("offline", self.pairing))
        self.assertEqual(api.call_args_list[-1].args[2]["offset"], 13)

    def test_paired_rerun_never_polls_or_rewrites_configuration(self):
        CONNECT.update_settings({
            "SYSGUD_ALLOWED_TELEGRAM_USER_IDS": "17,42", "TELEGRAM_ALLOWLIST": "17,42",
            "TELEGRAM_CHAT_ID": "42", "SYSGUD_TELEGRAM_ENABLED": "true",
        })
        self.pairing.update(paired=True, actor_id=42)
        self.pairing_path.write_text(json.dumps(self.pairing), encoding="utf-8")
        original = self.env.read_bytes()
        def respond(token, method, body):
            if method == "getMe":
                return {"username": "offline_bot", "id": 500, "is_bot": True}
            if method == "setMyCommands":
                return True
            raise AssertionError("Already paired must not call " + method)
        with mock.patch.object(CONNECT, "telegram", side_effect=respond), \
                mock.patch.object(CONNECT.sys, "argv", ["connect-telegram.py"]), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(CONNECT.main(), 0)
        self.assertTrue(json.loads(output.getvalue())["paired"])
        self.assertEqual(self.env.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
