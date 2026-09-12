#!/usr/bin/env python3
"""Behavioral checks for the disposable demo's host and container contracts."""

from __future__ import annotations

import contextlib
import http.server
import importlib.util
import io
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
HEALTHCHECK_PATH = ROOT / "demo" / "healthcheck.py"


class _Server(http.server.ThreadingHTTPServer):
    allow_reuse_address = True


class _Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, _format: str, *_args: object) -> None:
        pass

    def do_GET(self) -> None:
        mode = self.server.mode
        self.server.paths.append(self.path)
        if self.path == "/health" and mode.startswith("sutura"):
            self._reply(200, {"status": "ok"})
        elif self.path == "/openapi.json":
            paths = {"/v1/catalog": {"get": {}}, "/v1/query": {"post": {}}}
            if mode == "sutura-missing-operation":
                del paths["/v1/query"]
            elif mode == "sutura-extra-operation":
                paths["/v1/admin"] = {"get": {}}
            self._reply(200, {"paths": paths})
        elif self.path == "/health" and mode.startswith("webui"):
            self._reply(200, {"status": True})
        elif self.path == "/api/v1/tools/" and mode.startswith("webui"):
            self.server.webui_authorization.append(self.headers.get("Authorization"))
            if self.headers.get("Authorization") != "Bearer demo-session":
                self._reply(401, {"detail": "Not authenticated"})
            elif mode == "webui-missing-registration":
                self._reply(200, [{"id": "server:other"}])
            elif mode == "webui-extra-registration":
                self._reply(200, [{"id": "server:sutura"}, {"id": "server:other"}])
            else:
                self._reply(200, [{"id": "server:sutura"}])
        elif self.path == "/models" and mode in {"model", "redirect"}:
            self.server.authorization.append(self.headers.get("Authorization"))
            if mode == "model":
                self._reply(200, {"data": []})
            else:
                self.send_response(302)
                self.send_header("Location", "/elsewhere")
                self.end_headers()
        else:
            self.send_error(404)

    def do_POST(self) -> None:
        self.server.paths.append(self.path)
        if self.path == "/api/v1/auths/signin" and self.server.mode.startswith("webui"):
            if self.server.mode == "webui-auth-fail":
                self._reply(401, {"detail": "Invalid credentials"})
            else:
                self._reply(200, {"token": "demo-session", "token_type": "Bearer"})
        else:
            self.send_error(404)

    def _reply(self, status: int, value: object) -> None:
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


@contextlib.contextmanager
def server(mode: str):
    instance = _Server(("127.0.0.1", 0), _Handler)
    instance.mode = mode
    instance.authorization = []
    instance.webui_authorization = []
    instance.paths = []
    thread = threading.Thread(target=instance.serve_forever, daemon=True)
    thread.start()
    try:
        yield instance.server_address[1], instance
    finally:
        instance.shutdown()
        thread.join()
        instance.server_close()


def load_healthcheck():
    spec = importlib.util.spec_from_file_location("demo_healthcheck", HEALTHCHECK_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def launcher_fakes(
    root: pathlib.Path,
) -> tuple[pathlib.Path, pathlib.Path, dict[str, str]]:
    fake_bin = root / "bin"
    fake_bin.mkdir()
    log = root / "cargo.log"
    image = root / "serve-image"
    image.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    image.chmod(0o755)
    (fake_bin / "nix").write_text(
        f"#!/bin/sh\nprintf '%s\\n' {image}\n", encoding="utf-8"
    )
    (fake_bin / "docker").write_text(
        f"#!/bin/sh\nprintf '%s\\n' \"$*\" >> {root / 'docker.log'}\n",
        encoding="utf-8",
    )
    (fake_bin / "just").write_text(
        "#!/bin/sh\nprintf '127.0.0.1:9\\n'\n", encoding="utf-8"
    )
    (fake_bin / "cargo").write_text(
        f"#!/bin/sh\nprintf '%s\\n' \"$*\" >> {log}\nexit 0\n",
        encoding="utf-8",
    )
    for executable in fake_bin.iterdir():
        executable.chmod(0o755)
    return fake_bin, log, {"PATH": f"{fake_bin}:{os.environ['PATH']}"}


class DemoBehavior(unittest.TestCase):
    def test_transport_policy_refuses_cleartext_or_missing_credentials_without_echoing(
        self,
    ) -> None:
        for endpoint, key in (
            ("http://api.example.com/v1", ""),
            ("http://host.docker.internal:11434/v1", "test-key"),
            ("https://api.example.com/v1", ""),
        ):
            environment = {
                "SUTURA_DEMO_MODEL_ENDPOINT": endpoint,
                "SUTURA_DEMO_MODEL": "test-model",
                "SUTURA_DEMO_MODEL_API_KEY": key,
                "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
            }
            result = subprocess.run(
                ["bash", str(ROOT / "demo/start.sh"), "--check"],
                cwd=ROOT,
                env={**os.environ, **environment},
                capture_output=True,
                text=True,
                check=False,
            )
            output = result.stdout + result.stderr
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(endpoint, output)
            if key:
                self.assertNotIn(key, output)

    def test_endpoint_userinfo_is_refused_without_echoing_endpoint_or_key(self) -> None:
        for endpoint in (
            "http://localhost:11434@api.example.com/v1",
            "http://127.0.0.1:11434@api.example.com/v1",
        ):
            with tempfile.TemporaryDirectory() as directory:
                _fake_bin, cargo_log, fake_env = launcher_fakes(pathlib.Path(directory))
                environment = {
                    **fake_env,
                    "SUTURA_DEMO_MODEL_ENDPOINT": endpoint,
                    "SUTURA_DEMO_MODEL": "test-model",
                    "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                    "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
                }
                result = subprocess.run(
                    ["bash", str(ROOT / "demo/start.sh"), "--up-only"],
                    cwd=ROOT,
                    env={**os.environ, **environment},
                    capture_output=True,
                    text=True,
                    check=False,
                )
                output = result.stdout + result.stderr
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn(endpoint, output)
                self.assertNotIn("test-key", output)
                self.assertFalse(cargo_log.exists())

    def test_loopback_ipv6_is_accepted_and_not_echoed(self) -> None:
        environment = {
            "SUTURA_DEMO_MODEL_ENDPOINT": "http://[::1]:11434/v1",
            "SUTURA_DEMO_MODEL": "test-model",
            "SUTURA_DEMO_MODEL_API_KEY": "",
            "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
        }
        result = subprocess.run(
            ["bash", str(ROOT / "demo/start.sh"), "--check"],
            cwd=ROOT,
            env={**os.environ, **environment},
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(
            environment["SUTURA_DEMO_MODEL_ENDPOINT"], result.stdout + result.stderr
        )

    def test_remote_ipv6_keeps_its_brackets_in_the_container_endpoint(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fake_bin, _cargo_log, fake_env = launcher_fakes(root)
            endpoint_log = root / "endpoint"
            (fake_bin / "docker").write_text(
                f"#!/bin/sh\nprintf '%s\\n' \"$SUTURA_DEMO_MODEL_ENDPOINT\" > {endpoint_log}\n",
                encoding="utf-8",
            )
            (fake_bin / "docker").chmod(0o755)
            endpoint = "https://[2001:db8::1]:8443/v1"
            environment = {
                **fake_env,
                "SUTURA_DEMO_MODEL_ENDPOINT": endpoint,
                "SUTURA_DEMO_MODEL": "test-model",
                "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
            }
            result = subprocess.run(
                ["bash", str(ROOT / "demo/start.sh"), "--up-only"],
                cwd=ROOT,
                env={**os.environ, **environment},
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(endpoint_log.read_text(encoding="utf-8"), f"{endpoint}\n")
            self.assertNotIn("test-key", result.stdout + result.stderr)

    def test_build_uses_a_named_server_context_and_worktree_keyed_tag(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            _fake_bin, _cargo_log, fake_env = launcher_fakes(root)
            environment = {
                **fake_env,
                "SUTURA_DEMO_MODEL_ENDPOINT": "https://api.example.com/v1",
                "SUTURA_DEMO_MODEL": "test-model",
                "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
            }
            result = subprocess.run(
                ["bash", str(ROOT / "demo/start.sh"), "--up-only"],
                cwd=ROOT,
                env={**os.environ, **environment},
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            docker_args = (root / "docker.log").read_text(encoding="utf-8")
            self.assertIn("--build-context sutura-server=", docker_args)
            self.assertIn("/sutura/local-chat-demo:0.11.3-", docker_args)
            self.assertNotIn("sutura-serve:latest", docker_args)
            self.assertNotIn("load", docker_args)
            self.assertNotIn("test-key", result.stdout + result.stderr)

    def _run_healthcheck(
        self,
        model_mode: str,
        key: str,
        webui_mode: str = "webui",
        sutura_mode: str = "sutura",
    ):
        healthcheck = load_healthcheck()
        with (
            server(sutura_mode) as (sutura_port, sutura_server),
            server(webui_mode) as (webui_port, webui_server),
            server(model_mode) as (model_port, model_server),
            tempfile.TemporaryDirectory() as directory,
        ):
            pathlib.Path(directory, "token").write_text(
                "deployment-token", encoding="utf-8"
            )
            old = os.environ.copy()
            os.environ.update(
                {
                    "SUTURA_DEMO_SUTURA_PORT": str(sutura_port),
                    "SUTURA_DEMO_WEBUI_PORT": str(webui_port),
                    "SUTURA_DEMO_MODEL_ENDPOINT": f"http://127.0.0.1:{model_port}",
                    "SUTURA_DEMO_MODEL_API_KEY": key,
                    "SUTURA_DEMO_RUN_DIR": directory,
                }
            )
            stdout = io.StringIO()
            try:
                with contextlib.redirect_stdout(stdout):
                    healthcheck.main()
            finally:
                os.environ.clear()
                os.environ.update(old)
            return (
                stdout.getvalue(),
                model_server.authorization,
                sutura_server.paths,
                webui_server.paths,
                webui_server.webui_authorization,
            )

    def test_keyless_model_probe_sends_no_authorization_and_checks_tools(self) -> None:
        output, authorization, sutura_paths, webui_paths, webui_authorization = (
            self._run_healthcheck("model", "")
        )
        self.assertEqual(
            output,
            "healthcheck: the sutura server, its two operations and the chat client are all up\n",
        )
        self.assertEqual(authorization, [None])
        self.assertIn("/api/v1/auths/signin", webui_paths)
        self.assertIn("/api/v1/tools/", webui_paths)
        self.assertEqual(webui_authorization, ["Bearer demo-session"])
        self.assertIn("/openapi.json", sutura_paths)

    def test_registry_authentication_failure_fails_readiness(self) -> None:
        healthcheck = load_healthcheck()
        with (
            server("sutura") as (sutura_port, _),
            server("webui-auth-fail") as (webui_port, _),
            server("model") as (model_port, _),
            tempfile.TemporaryDirectory() as directory,
        ):
            pathlib.Path(directory, "token").write_text(
                "deployment-token", encoding="utf-8"
            )
            old = os.environ.copy()
            os.environ.update(
                {
                    "SUTURA_DEMO_SUTURA_PORT": str(sutura_port),
                    "SUTURA_DEMO_WEBUI_PORT": str(webui_port),
                    "SUTURA_DEMO_MODEL_ENDPOINT": f"http://127.0.0.1:{model_port}",
                    "SUTURA_DEMO_RUN_DIR": directory,
                }
            )
            try:
                with self.assertRaises(SystemExit) as raised:
                    healthcheck.main()
            finally:
                os.environ.clear()
                os.environ.update(old)
            self.assertEqual(raised.exception.code, 1)

    def test_missing_registry_registration_fails_readiness(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            self._run_healthcheck("model", "", webui_mode="webui-missing-registration")
        self.assertEqual(raised.exception.code, 1)

    def test_extra_or_missing_served_operation_fails_readiness(self) -> None:
        for mode in ("sutura-extra-operation", "sutura-missing-operation"):
            stderr = io.StringIO()
            with (
                self.subTest(mode=mode),
                contextlib.redirect_stderr(stderr),
                self.assertRaises(SystemExit) as raised,
            ):
                self._run_healthcheck("model", "", sutura_mode=mode)
            self.assertEqual(raised.exception.code, 1)
            self.assertIn("served document operations", stderr.getvalue())

    def test_keyed_model_probe_sends_exact_bearer_and_redirect_is_not_followed(
        self,
    ) -> None:
        _output, authorization, _sutura_paths, _webui_paths, _webui_authorization = (
            self._run_healthcheck("model", "test-key")
        )
        self.assertEqual(authorization, ["Bearer test-key"])
        healthcheck = load_healthcheck()
        with (
            server("sutura") as (sutura_port, _),
            server("webui") as (webui_port, _),
            server("redirect") as (model_port, model_server),
            tempfile.TemporaryDirectory() as directory,
        ):
            pathlib.Path(directory, "token").write_text(
                "deployment-token", encoding="utf-8"
            )
            old = os.environ.copy()
            os.environ.update(
                {
                    "SUTURA_DEMO_SUTURA_PORT": str(sutura_port),
                    "SUTURA_DEMO_WEBUI_PORT": str(webui_port),
                    "SUTURA_DEMO_MODEL_ENDPOINT": f"http://127.0.0.1:{model_port}",
                    "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                    "SUTURA_DEMO_RUN_DIR": directory,
                }
            )
            stderr = io.StringIO()
            try:
                with (
                    contextlib.redirect_stderr(stderr),
                    self.assertRaises(SystemExit) as raised,
                ):
                    healthcheck.main()
            finally:
                os.environ.clear()
                os.environ.update(old)
            self.assertEqual(raised.exception.code, 1)
            self.assertEqual(model_server.authorization, ["Bearer test-key"])
            self.assertEqual(model_server.paths, ["/models"])
            self.assertNotIn("test-key", stderr.getvalue())

    def test_redirect_refusing_opener_rejects_redirects(self) -> None:
        healthcheck = load_healthcheck()
        with server("redirect") as (model_port, _):
            request = healthcheck.urllib.request.Request(
                f"http://127.0.0.1:{model_port}/models"
            )
            with self.assertRaises(healthcheck.urllib.error.HTTPError) as raised:
                healthcheck.NO_REDIRECT.open(request, timeout=4)
            self.assertEqual(raised.exception.code, 302)

    def test_supervisor_passes_exact_model_key_to_webui_child(self) -> None:
        source = (ROOT / "demo/run.sh").read_text(encoding="utf-8")
        self.assertIn("/usr/local/bin/sutura-serve", source)
        self.assertIn("/app/backend", source)
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            server_path = root / "serve"
            server_path.write_text(
                "#!/bin/sh\ntrap 'exit 0' TERM\nwhile :; do sleep 1; done\n",
                encoding="utf-8",
            )
            server_path.chmod(0o755)
            backend = root / "backend"
            backend.mkdir()
            (backend / "start.sh").write_text(
                '#!/bin/sh\nprintf \'%s\\n\' "$OPENAI_API_KEY" > "$SUTURA_DEMO_RUN_DIR/key"\n'
                'printf \'%s\\n\' "$ENABLE_PERSISTENT_CONFIG" > "$SUTURA_DEMO_RUN_DIR/persistent"\n'
                "exit 0\n",
                encoding="utf-8",
            )
            (backend / "start.sh").chmod(0o755)
            instrumented = root / "run.sh"
            instrumented.write_text(
                source.replace("/usr/local/bin/sutura-serve", str(server_path)).replace(
                    "/app/backend", str(backend)
                ),
                encoding="utf-8",
            )
            refused_dir = root / "run-refused"
            refused_dir.mkdir()
            refused_key = "must-not-reach-child"
            refused = subprocess.run(
                ["bash", str(instrumented)],
                cwd=ROOT,
                env={
                    **os.environ,
                    "SUTURA_DEMO_MODEL_ENDPOINT": "http://host.docker.internal:11434/v1",
                    "SUTURA_DEMO_MODEL": "test-model",
                    "SUTURA_DEMO_MODEL_API_KEY": refused_key,
                    "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
                    "SUTURA_DEMO_RUN_DIR": str(refused_dir),
                },
                capture_output=True,
                text=True,
                check=False,
                timeout=5,
            )
            self.assertNotEqual(refused.returncode, 0)
            self.assertFalse((refused_dir / "key").exists())
            self.assertNotIn(refused_key, refused.stdout + refused.stderr)
            for key in ("", "exact-test-key"):
                run_dir = root / ("run-empty" if not key else "run-keyed")
                run_dir.mkdir()
                environment = {
                    "SUTURA_DEMO_MODEL_ENDPOINT": "https://host.docker.internal:11434/v1",
                    "SUTURA_DEMO_MODEL": "test-model",
                    "SUTURA_DEMO_MODEL_API_KEY": key,
                    "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
                    "SUTURA_DEMO_RUN_DIR": str(run_dir),
                    "SUTURA_DEMO_SUTURA_PORT": "9",
                    "SUTURA_DEMO_WEBUI_PORT": "8",
                }
                bash = os.environ.get("BASH", "bash")
                bash_path = shutil.which(bash) or bash
                result = subprocess.run(
                    [bash_path, str(instrumented)],
                    cwd=ROOT,
                    env={**os.environ, **environment},
                    capture_output=True,
                    text=True,
                    check=False,
                    timeout=5,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(
                    (run_dir / "key").read_text(encoding="utf-8"), f"{key}\n"
                )
                self.assertEqual(
                    (run_dir / "persistent").read_text(encoding="utf-8"), "false\n"
                )
                if key:
                    self.assertNotIn(key, result.stdout)
                    self.assertNotIn(key, result.stderr)


if __name__ == "__main__":
    unittest.main()
