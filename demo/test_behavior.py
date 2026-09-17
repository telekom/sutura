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
import signal
import subprocess
import tempfile
import threading
import time
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
        elif self.path == "/v1/query" and self.server.mode.startswith("sutura"):
            length = int(self.headers.get("Content-Length", "0"))
            question = json.loads(self.rfile.read(length))
            self.server.queries.append(question)
            if (
                question.get("metric") == "customer_lifetime_value"
                and self.server.mode != "sutura-always-answers"
            ):
                self._reply(404, {"outcome": "refusal", "reason": "metric_unknown"})
            else:
                self._reply(200, {"outcome": "answer", "rows": [[1]]})
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
    instance.queries = []
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


def supervised_children(
    root: pathlib.Path, serve_script: str, backend_script: str
) -> pathlib.Path:
    """`demo/run.sh`, with its two hard-coded child paths swapped for scripts this test controls."""
    server_path = root / "serve"
    server_path.write_text(serve_script, encoding="utf-8")
    server_path.chmod(0o755)
    backend = root / "backend"
    backend.mkdir()
    (backend / "start.sh").write_text(backend_script, encoding="utf-8")
    (backend / "start.sh").chmod(0o755)
    source = (ROOT / "demo/run.sh").read_text(encoding="utf-8")
    instrumented = root / "run.sh"
    instrumented.write_text(
        # The fake ignores its own arguments, so leaving ` serve` in place after the binary path is
        # swapped is harmless - it is invoked as `<fake> serve &`, same as the real supervisor
        # invokes `sutura serve &` (`github.com/telekom/sutura#685` step 2 folded the separate
        # `sutura-serve` binary this used to name into a subcommand of `sutura`).
        source.replace("/usr/local/bin/sutura", str(server_path)).replace(
            "/app/backend", str(backend)
        ),
        encoding="utf-8",
    )
    return instrumented


def supervisor_environment(run_dir: pathlib.Path) -> dict[str, str]:
    """The minimum env `demo/run.sh` needs to reach its two children, ports it never binds."""
    return {
        "SUTURA_DEMO_MODEL_ENDPOINT": "https://host.docker.internal:11434/v1",
        "SUTURA_DEMO_MODEL": "test-model",
        "SUTURA_DEMO_MODEL_API_KEY": "test-key",
        "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
        "SUTURA_DEMO_RUN_DIR": str(run_dir),
        "SUTURA_DEMO_SUTURA_PORT": "9",
        "SUTURA_DEMO_WEBUI_PORT": "8",
    }


def _wait_for(path: pathlib.Path, seconds: float = 15.0) -> None:
    """Poll for a file rather than sleep a fixed guess - a freshly written script's first exec can
    take longer than any short sleep on a loaded host, and a blind sleep just makes that flaky."""
    deadline = time.monotonic() + seconds
    while not path.exists():
        if time.monotonic() > deadline:
            raise AssertionError(f"{path} never appeared")
        time.sleep(0.02)


# `touch {ready}` lets a caller wait for the trap to be INSTALLED rather than guessing a sleep is
# long enough - the failure mode a blind sleep hides is the trap never running at all, which reads
# identically to "hasn't happened yet" until the deadline above catches it.
_LOOP_UNTIL_TERM_THEN_MARK = (
    "#!/bin/sh\n"
    "trap 'printf terminated > {marker}; exit 143' TERM\n"
    "touch {ready}\n"
    "while :; do sleep 1; done\n"
)
_WAIT_FOR_READY_THEN_EXIT = (
    "#!/bin/sh\nwhile [ ! -f {ready} ]; do sleep 0.02; done\nexit {code}\n"
)


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
            self.assertIn("/sutura/local-chat-demo:demo-", docker_args)
            self.assertNotIn("sutura:latest", docker_args)
            self.assertNotIn("load", docker_args)
            self.assertNotIn("test-key", result.stdout + result.stderr)

    def test_dev_down_only_demo_scopes_the_docker_teardown_it_issues(self) -> None:
        """The CLI DISPATCH, not the pure `teardown::plan`/`scoped_down_args` functions their own
        unit tests already cover: this runs the real `xtask dev-down --only demo` binary against a
        docker stub that only logs its argv, and reads back the exact `docker compose ... down`
        invocation the CLI issued.

        Proves the arguments a real docker call would have received - scoped to the `demo` service,
        with no `--volumes` - never the whole-project `down --volumes --remove-orphans` teardown.rs's
        own tests hold for a bare `dev-down`. Does NOT prove a neighbour's container or volume
        survives: the stub has no container semantics, so there is no second container here for a
        teardown to spare."""
        with tempfile.TemporaryDirectory() as directory:
            fake_bin = pathlib.Path(directory) / "bin"
            fake_bin.mkdir()
            docker_log = pathlib.Path(directory) / "docker.log"
            (fake_bin / "docker").write_text(
                f"#!/bin/sh\nprintf '%s\\n' \"$*\" >> {docker_log}\n", encoding="utf-8"
            )
            (fake_bin / "docker").chmod(0o755)
            result = subprocess.run(
                [
                    "cargo",
                    "run",
                    "-q",
                    "-p",
                    "xtask",
                    "--",
                    "dev-down",
                    "--only",
                    "demo",
                ],
                cwd=ROOT,
                env={**os.environ, "PATH": f"{fake_bin}:{os.environ['PATH']}"},
                capture_output=True,
                text=True,
                check=False,
                timeout=300,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = [
                line.split()
                for line in docker_log.read_text(encoding="utf-8").splitlines()
                if "down" in line.split()
            ]
            self.assertEqual(len(calls), 1, calls)
            teardown_call = calls[0]
            self.assertEqual(
                teardown_call[-2:],
                ["down", "demo"],
                "the CLI must issue the scoped teardown - a service name after `down`, never the "
                f"whole-project `--volumes --remove-orphans` form: {teardown_call}",
            )
            self.assertNotIn("--volumes", teardown_call)
            self.assertIn("--profile", teardown_call)

    def _run_healthcheck(
        self,
        model_mode: str,
        key: str,
        webui_mode: str = "webui",
        sutura_mode: str = "sutura",
        probes: int = 1,
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
                    for _ in range(probes):
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

    def test_smoke_test_asks_a_real_question_and_provokes_a_refusal(self) -> None:
        # The registration half (above) proves the document is served; this proves it is USED -
        # the answer half and the refusal half of the acceptance matrix's in-container smoke test,
        # both against the shipped corpus rather than an invented question.
        healthcheck = load_healthcheck()
        with (
            server("sutura") as (sutura_port, sutura_server),
            server("webui") as (webui_port, _),
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
                with contextlib.redirect_stdout(io.StringIO()):
                    healthcheck.main()
            finally:
                os.environ.clear()
                os.environ.update(old)
            metrics = [query["metric"] for query in sutura_server.queries]
            self.assertIn("active_subscriptions", metrics)
            self.assertIn("customer_lifetime_value", metrics)

    def test_a_question_that_should_be_refused_but_is_answered_fails_readiness(
        self,
    ) -> None:
        # The refusal half's own negative control: a demo that ANSWERS an unanswerable question -
        # the failure this smoke test exists to catch - must fail readiness rather than pass it.
        with self.assertRaises(SystemExit) as raised:
            self._run_healthcheck("model", "", sutura_mode="sutura-always-answers")
        self.assertEqual(raised.exception.code, 1)

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
        self.assertIn("/usr/local/bin/sutura serve", source)
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
                # The fake ignores its own arguments, so leaving ` serve` in place after the binary
                # path is swapped is harmless - it is invoked as `<fake> serve &`, same as the real
                # supervisor invokes `sutura serve &`.
                source.replace("/usr/local/bin/sutura", str(server_path)).replace(
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

    def test_a_normal_double_exit_still_ends_the_container_unhealthy(self) -> None:
        # "A child that exited 0 is still the demo ending" - `demo/run.sh`'s own comment. The
        # server exits first; `wait -n` catches status 0, `terminate` reaches the backend while it
        # is also on its way out, and the forced-to-1 rule is what must survive both exiting clean.
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            instrumented = supervised_children(
                root,
                "#!/bin/sh\nsleep 0.2\nexit 0\n",
                "#!/bin/sh\ntrap 'exit 0' TERM\nsleep 0.4\nexit 0\n",
            )
            result = subprocess.run(
                ["bash", str(instrumented)],
                cwd=ROOT,
                env={**os.environ, **supervisor_environment(root / "run")},
                capture_output=True,
                text=True,
                check=False,
                timeout=10,
            )
            self.assertEqual(result.returncode, 1, result.stderr)

    def test_either_child_exiting_terminates_the_other_and_carries_its_own_code(
        self,
    ) -> None:
        for first in ("serve", "backend"):
            with self.subTest(first=first), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                marker = root / "survivor-marker"
                ready = root / "survivor-ready"
                # The exiting side waits for the survivor's trap to be INSTALLED rather than
                # exiting on a timer - a fixed sleep here is exactly the race that let this
                # scenario go unexercised: a freshly written script's first exec is not bounded.
                exiting = _WAIT_FOR_READY_THEN_EXIT.format(ready=ready, code=7)
                surviving = _LOOP_UNTIL_TERM_THEN_MARK.format(
                    marker=marker, ready=ready
                )
                scripts = (
                    (exiting, surviving) if first == "serve" else (surviving, exiting)
                )
                instrumented = supervised_children(root, *scripts)
                result = subprocess.run(
                    ["bash", str(instrumented)],
                    cwd=ROOT,
                    env={**os.environ, **supervisor_environment(root / "run")},
                    capture_output=True,
                    text=True,
                    check=False,
                    timeout=20,
                )
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertEqual(marker.read_text(encoding="utf-8"), "terminated")

    def test_an_interrupt_terminates_both_children_and_exits_143(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            serve_marker = root / "serve-marker"
            serve_ready = root / "serve-ready"
            backend_marker = root / "backend-marker"
            backend_ready = root / "backend-ready"
            instrumented = supervised_children(
                root,
                _LOOP_UNTIL_TERM_THEN_MARK.format(
                    marker=serve_marker, ready=serve_ready
                ),
                _LOOP_UNTIL_TERM_THEN_MARK.format(
                    marker=backend_marker, ready=backend_ready
                ),
            )
            process = subprocess.Popen(
                ["bash", str(instrumented)],
                cwd=ROOT,
                env={**os.environ, **supervisor_environment(root / "run")},
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            try:
                _wait_for(serve_ready)
                _wait_for(backend_ready)
                process.send_signal(signal.SIGTERM)
                _stdout, stderr = process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                raise
            self.assertEqual(process.returncode, 143, stderr)
            self.assertEqual(serve_marker.read_text(encoding="utf-8"), "terminated")
            self.assertEqual(backend_marker.read_text(encoding="utf-8"), "terminated")

    def test_repeated_probes_present_a_credential_exactly_once(self) -> None:
        # THE COUNT IS THE TEST, never the stamp file: a test asserting only that the stamp exists
        # passes with the pump still running. Compose probes this container every three seconds, so
        # a probe that re-signed in and re-sent the operator's model key on each one put that key
        # on the wire roughly twelve hundred times an hour for as long as the demo ran.
        _output, authorization, sutura_paths, webui_paths, webui_authorization = (
            self._run_healthcheck("model", "test-key", probes=3)
        )
        self.assertEqual(authorization, ["Bearer test-key"])
        self.assertEqual(webui_authorization, ["Bearer demo-session"])
        self.assertEqual(webui_paths.count("/api/v1/auths/signin"), 1)
        self.assertEqual(webui_paths.count("/api/v1/tools/"), 1)
        # What the latch does NOT give up, and so is still counted per probe: the server's own
        # liveness and the shape of the document the chat client reads.
        self.assertEqual(sutura_paths.count("/health"), 3)
        self.assertEqual(sutura_paths.count("/openapi.json"), 3)

    def test_an_empty_deployment_token_refuses_readiness_and_is_never_presented(
        self,
    ) -> None:
        # A zero-byte token file reads back as "", the server reads `access_token: ""` as ABSENT
        # and serves this loopback development bind ungated by design - so nothing downstream
        # refuses, and the probe reported the demo READY while it held no credential at all.
        healthcheck = load_healthcheck()
        with (
            server("sutura") as (sutura_port, sutura_server),
            server("webui") as (webui_port, _),
            server("model") as (model_port, model_server),
            tempfile.TemporaryDirectory() as directory,
        ):
            token_file = pathlib.Path(directory, "token")
            token_file.write_text("", encoding="utf-8")
            self.assertEqual(token_file.stat().st_size, 0)
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
            self.assertIn("holds no credential", stderr.getvalue())
            # The empty bearer never reached the served document, and the probe did not go on to
            # present the operator's model key on behalf of a demo that authenticates nobody.
            self.assertNotIn("/openapi.json", sutura_server.paths)
            self.assertEqual(model_server.authorization, [])

    def test_a_token_generator_that_produces_nothing_refuses_to_serve(self) -> None:
        # The guard this holds rested on `set -e` alone: it called a `fail` this script never
        # defines, so the refusal was a `command not found` whose 127 only `-e` turned into an
        # exit. With `-e` removed the same guard wrote a zero-byte token file AND `access_token:
        # ""` into the deployment. Neither file may be produced, and the reason must be the one
        # the operator can act on.
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            silent = fake_bin / "python3"
            silent.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            silent.chmod(0o755)
            run_dir = root / "run"
            result = subprocess.run(
                ["bash", str(ROOT / "demo/run.sh")],
                cwd=ROOT,
                env={
                    **os.environ,
                    "PATH": f"{fake_bin}:{os.environ['PATH']}",
                    "SUTURA_DEMO_MODEL_ENDPOINT": "https://host.docker.internal:11434/v1",
                    "SUTURA_DEMO_MODEL": "test-model",
                    "SUTURA_DEMO_MODEL_API_KEY": "test-key",
                    "SUTURA_DEMO_ACKNOWLEDGE": "one local test user",
                    "SUTURA_DEMO_RUN_DIR": str(run_dir),
                },
                capture_output=True,
                text=True,
                check=False,
                timeout=30,
            )
            output = result.stdout + result.stderr
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((run_dir / "token").exists())
            self.assertFalse((run_dir / "base.yaml").exists())
            self.assertIn("token generator produced nothing", output)
            # Locale-independent: bash names the missing command as `fail:` in every locale, so
            # this is the assertion that fails if the guard goes back to calling one.
            self.assertNotIn("fail:", output)
            self.assertNotIn("test-key", output)


if __name__ == "__main__":
    unittest.main()
