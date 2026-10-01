#!/usr/bin/env python3
"""Behavioral checks for `just demo-mcp`: the Keycloak issuer wired in, and `/mcp` held behind it."""

from __future__ import annotations

import base64
import contextlib
import http.server
import json
import os
import pathlib
import secrets
import subprocess
import sys
import tempfile
import threading
import unittest

from test_behavior import HANG_CEILING_S, ROOT, launcher_fakes, supervised_children

DEMO = ROOT / "examples" / "demo-chatinterface"
ISSUER = "https://127.0.0.1:8443/realms/sutura-dev"
RESOURCE = "https://sutura-dev-cli.example.com"
KEY_SET = '{"keys":[{"kid":"k","kty":"RSA","n":"AQAB","e":"AQAB"}]}'
TOKEN = "header.minted-by-the-tier.signature"
# Generated per run, like the tier's own: a refusal that echoed one would show it in its stderr.
CLIENT_SECRET = secrets.token_urlsafe(16)
SUBJECT_PASSWORD = secrets.token_urlsafe(16)


class _Server(http.server.ThreadingHTTPServer):
    allow_reuse_address = True


class _Handler(http.server.BaseHTTPRequestHandler):
    """One fake for every endpoint the demo's probe and minting helper reach; `mode` picks a defect."""

    def log_message(self, _format: str, *_args: object) -> None:
        pass

    def do_GET(self) -> None:
        if self.path == "/health":
            self._reply(200, {"status": "ok"})
        elif self.path == "/models":
            self._reply(200, {"data": []})
        elif self.path == "/api/v1/tools/":
            self._reply(200, [{"id": "server:mcp:sutura"}])
        elif self.path.endswith("/protocol/openid-connect/certs"):
            self._reply(200, json.loads(KEY_SET))
        else:
            self.send_error(404)

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        mode = self.server.mode
        if self.path == "/api/v1/auths/signin":
            self._reply(200, {"token": "demo-session"})
        elif self.path == "/mcp":
            presented = self.headers.get("Authorization")
            self.server.mcp_authorization.append(presented)
            if presented != f"Bearer {TOKEN}" and not (
                presented is None and mode == "mcp-open"
            ):
                self._reply(401, {"code": "unauthorized"})
            elif json.loads(body)["method"] == "tools/list":
                tools = (
                    []
                    if mode == "mcp-lists-nothing"
                    else [{"name": "describe_catalog"}]
                )
                self._reply(
                    200, {"jsonrpc": "2.0", "id": 1, "result": {"tools": tools}}
                )
            else:
                self._reply(200, {"jsonrpc": "2.0", "id": 1, "result": {}})
        elif self.path.endswith("/protocol/openid-connect/token"):
            self.server.grants.append(body.decode())
            if mode == "token-missing":
                self._reply(200, {"error": "nothing here"})
            else:
                self._reply(200, {"access_token": self.server.minted})
        else:
            self.send_error(404)

    def _reply(self, status: int, value: object) -> None:
        payload = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


@contextlib.contextmanager
def fake(mode: str = "honest", minted: str = TOKEN):
    instance = _Server(("127.0.0.1", 0), _Handler)
    instance.mode = mode
    instance.minted = minted
    instance.mcp_authorization = []
    instance.grants = []
    thread = threading.Thread(target=instance.serve_forever, daemon=True)
    thread.start()
    try:
        yield instance.server_address[1], instance
    finally:
        instance.shutdown()
        thread.join()
        instance.server_close()


def mcp_environment(run_dir: pathlib.Path, **override: str) -> dict[str, str]:
    return {
        **os.environ,
        "SUTURA_DEMO_MODEL_ENDPOINT": "https://host.docker.internal:11434/v1",
        "SUTURA_DEMO_MODEL": "test-model",
        "SUTURA_DEMO_MODEL_API_KEY": "test-key",
        "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
        "SUTURA_DEMO_RUN_DIR": str(run_dir),
        "SUTURA_DEMO_SUTURA_PORT": "9",
        "SUTURA_DEMO_WEBUI_PORT": "8",
        "SUTURA_DEMO_SURFACE": "mcp",
        "SUTURA_DEMO_MCP_ISSUER": ISSUER,
        "SUTURA_DEMO_MCP_RESOURCE": RESOURCE,
        "SUTURA_DEMO_MCP_KEY_SET": KEY_SET,
        "SUTURA_DEMO_MCP_TOKEN": TOKEN,
        **override,
    }


def jwt(claims: dict[str, object]) -> str:
    payload = (
        base64.urlsafe_b64encode(json.dumps(claims).encode()).rstrip(b"=").decode()
    )
    return f"eyJhbGciOiJSUzI1NiJ9.{payload}.signature"


class DemoMcp(unittest.TestCase):
    def _supervise(self, root: pathlib.Path, environment: dict[str, str]):
        instrumented = supervised_children(
            root,
            "#!/bin/sh\nexit 0\n",
            '#!/bin/sh\nprintf \'%s\' "$TOOL_SERVER_CONNECTIONS" > "$SUTURA_DEMO_RUN_DIR/connections"\nexit 0\n',
        )
        return subprocess.run(
            ["bash", str(instrumented)],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
            timeout=HANG_CEILING_S,
        )

    def test_the_mcp_supervisor_wires_the_keycloak_issuer_and_registers_an_mcp_connection(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            run_dir = pathlib.Path(directory, "run")
            result = self._supervise(pathlib.Path(directory), mcp_environment(run_dir))
            self.assertNotIn(TOKEN, result.stdout + result.stderr)
            deployment = (run_dir / "base.yaml").read_text(encoding="utf-8")
            self.assertIn("  agent_surface:\n    enabled: true\n", deployment)
            self.assertIn(f'    authorization_server: "{ISSUER}"\n', deployment)
            self.assertIn(f'    resource: "{RESOURCE}"\n', deployment)
            self.assertIn(
                f'    key_set_file: "{run_dir}/keycloak-jwks.json"\n', deployment
            )
            self.assertIn('    mode: "direct"\n', deployment)
            self.assertNotIn("access_token", deployment)
            self.assertEqual(
                (run_dir / "keycloak-jwks.json").read_text(encoding="utf-8"), KEY_SET
            )
            self.assertEqual((run_dir / "token").read_text(encoding="utf-8"), TOKEN)
            (connection,) = json.loads(
                (run_dir / "connections").read_text(encoding="utf-8")
            )
            self.assertEqual(connection["type"], "mcp")
            self.assertEqual(connection["url"], "http://127.0.0.1:9/mcp")
            self.assertEqual(
                (connection["auth_type"], connection["key"]), ("bearer", TOKEN)
            )
            self.assertEqual(connection["info"]["id"], "sutura")

    def test_the_mcp_supervisor_refuses_a_missing_or_injecting_issuer_value(
        self,
    ) -> None:
        for name, value in (
            ("SUTURA_DEMO_MCP_TOKEN", ""),
            ("SUTURA_DEMO_MCP_KEY_SET", ""),
            ("SUTURA_DEMO_MCP_ISSUER", f'{ISSUER}"\n    algorithms: ["none"]'),
            ("SUTURA_DEMO_MCP_RESOURCE", f"{RESOURCE}\\"),
            ("SUTURA_DEMO_SURFACE", "graphql"),
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                run_dir = pathlib.Path(directory, "run")
                result = self._supervise(
                    pathlib.Path(directory), mcp_environment(run_dir, **{name: value})
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(name, result.stderr)
                self.assertFalse((run_dir / "base.yaml").exists())
                self.assertFalse((run_dir / "connections").exists())

    def _launch(self, arguments: list[str], **override: str):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = pathlib.Path(directory.name)
        fake_bin, _log, fake_env = launcher_fakes(root)
        # `nix` logs what it was asked, `cargo` records what `dev-up` would hand compose, and
        # `python3` stands in for the minting helper so no realm is read off this checkout.
        (fake_bin / "nix").write_text(
            f"#!/bin/sh\nprintf '%s\\n' \"$*\" >> {root / 'nix.log'}\nprintf '/nix/store/fake\\n'\n",
            encoding="utf-8",
        )
        (fake_bin / "cargo").write_text(
            "#!/bin/sh\nenv | grep '^SUTURA_DEMO_SURFACE=\\|^SUTURA_DEMO_MCP_' | sort "
            f">> {root / 'compose.env'}\nexit 0\n",
            encoding="utf-8",
        )
        (fake_bin / "python3").write_text(
            f"#!/bin/sh\nprintf '%s\\n' {ISSUER} {RESOURCE} {TOKEN} '{KEY_SET}'\n",
            encoding="utf-8",
        )
        for executable in fake_bin.iterdir():
            executable.chmod(0o755)
        result = subprocess.run(
            ["bash", str(DEMO / "start.sh"), *arguments],
            cwd=root,
            env={
                **os.environ,
                **fake_env,
                "SUTURA_DEMO_MODEL_ENDPOINT": "https://api.example.com/v1",
                "SUTURA_DEMO_MODEL": "test-model",
                "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
                **override,
            },
            capture_output=True,
            text=True,
            check=False,
            timeout=HANG_CEILING_S,
        )
        nix_log = root / "nix.log"
        compose_env = root / "compose.env"
        return (
            result,
            nix_log.read_text(encoding="utf-8") if nix_log.exists() else "",
            compose_env.read_text(encoding="utf-8") if compose_env.exists() else "",
        )

    def test_demo_mcp_starts_the_keycloak_tier_and_hands_its_token_to_compose(
        self,
    ) -> None:
        result, nix_log, compose_env = self._launch(["--mcp", "--up-only"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("run .#keycloak-tier -- start", nix_log)
        self.assertIn("SUTURA_DEMO_SURFACE=mcp\n", compose_env)
        self.assertIn(f"SUTURA_DEMO_MCP_ISSUER={ISSUER}\n", compose_env)
        self.assertIn(f"SUTURA_DEMO_MCP_RESOURCE={RESOURCE}\n", compose_env)
        self.assertIn(f"SUTURA_DEMO_MCP_TOKEN={TOKEN}\n", compose_env)
        self.assertIn(f"SUTURA_DEMO_MCP_KEY_SET={KEY_SET}\n", compose_env)
        self.assertNotIn(TOKEN, result.stdout + result.stderr)

    def test_plain_demo_stays_openapi_whatever_the_environment_says(self) -> None:
        result, nix_log, compose_env = self._launch(
            ["--up-only"], SUTURA_DEMO_SURFACE="mcp"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("keycloak-tier", nix_log)
        self.assertIn("SUTURA_DEMO_SURFACE=openapi\n", compose_env)

    def _probe(self, mode: str) -> subprocess.CompletedProcess:
        with (
            fake(mode) as (port, _instance),
            tempfile.TemporaryDirectory() as directory,
        ):
            pathlib.Path(directory, "token").write_text(TOKEN, encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(DEMO / "healthcheck.py")],
                env={
                    **os.environ,
                    "SUTURA_DEMO_SURFACE": "mcp",
                    "SUTURA_DEMO_SUTURA_PORT": str(port),
                    "SUTURA_DEMO_WEBUI_PORT": str(port),
                    "SUTURA_DEMO_MODEL_ENDPOINT": f"http://127.0.0.1:{port}",
                    "SUTURA_DEMO_MODEL_API_KEY": "",
                    "SUTURA_DEMO_RUN_DIR": directory,
                },
                capture_output=True,
                text=True,
                check=False,
                timeout=HANG_CEILING_S,
            )

    def test_readiness_holds_when_mcp_refuses_no_token_and_lists_for_the_issuers(
        self,
    ) -> None:
        result = self._probe("honest")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("/mcp surface", result.stdout)
        self.assertNotIn(TOKEN, result.stdout + result.stderr)

    def test_readiness_fails_when_mcp_answers_a_call_without_a_token(self) -> None:
        result = self._probe("mcp-open")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no token", result.stderr)

    def test_readiness_fails_when_mcp_lists_nothing_for_the_issuers_token(self) -> None:
        result = self._probe("mcp-lists-nothing")
        self.assertEqual(result.returncode, 1)
        self.assertIn("listed no tools", result.stderr)

    def _mint(
        self, mode: str, minted: str
    ) -> tuple[subprocess.CompletedProcess, list[str]]:
        with (
            fake(mode, minted) as (port, instance),
            tempfile.TemporaryDirectory() as directory,
        ):
            realm = pathlib.Path(directory, "realm.json")
            realm.write_text(
                json.dumps(
                    {
                        "issuer": f"http://127.0.0.1:{port}/realms/sutura-dev",
                        "tls_certificate_file": str(realm),
                        "client": {"id": "sutura-dev-cli", "secret": CLIENT_SECRET},
                        "subjects": [
                            {"username": "subject-a", "password": SUBJECT_PASSWORD}
                        ],
                    }
                ),
                encoding="utf-8",
            )
            result = subprocess.run(
                [sys.executable, str(DEMO / "keycloak_token.py"), str(realm)],
                capture_output=True,
                text=True,
                check=False,
                timeout=HANG_CEILING_S,
            )
            return result, instance.grants

    def test_the_minting_helper_prints_the_issuer_the_https_audience_the_token_and_the_key_set(
        self,
    ) -> None:
        token = jwt({"aud": ["account", RESOURCE]})
        result, grants = self._mint("honest", token)
        self.assertEqual(result.returncode, 0, result.stderr)
        issuer, audience, minted, key_set = result.stdout.splitlines()
        self.assertTrue(issuer.endswith("/realms/sutura-dev"))
        self.assertEqual(
            (audience, minted, json.loads(key_set)),
            (RESOURCE, token, json.loads(KEY_SET)),
        )
        self.assertIn("grant_type=password", grants[0])
        self.assertIn("username=subject-a", grants[0])

    def test_the_minting_helper_refuses_without_echoing_a_credential(self) -> None:
        for mode, minted, reason in (
            ("token-missing", TOKEN, "without an access token"),
            ("honest", jwt({"aud": ["account"]}), "no https:// audience"),
        ):
            with self.subTest(mode=mode):
                result, _grants = self._mint(mode, minted)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, "")
                self.assertIn(reason, result.stderr)
                for secret in (CLIENT_SECRET, SUBJECT_PASSWORD, minted):
                    self.assertNotIn(secret, result.stderr)


if __name__ == "__main__":
    unittest.main()
