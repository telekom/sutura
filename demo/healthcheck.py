#!/usr/bin/env python3
"""The demo's readiness probe: registration, an answer, a refusal, and the tool surface they exist for.

Compose runs this from `demo/Dockerfile`'s `HEALTHCHECK`. It is the demo's answer to the fact that
the shipped `sutura` image has no probe of its own: "the container is healthy" has to mean the
server answered, the chat client answered, the served document still exposes the two operations the
demo is about, AND the demo can actually answer a real question and refuse one it cannot - not just
that a document describing those operations is reachable. A client that is up while its server is
not would otherwise read as healthy, and the tier's provision would report success over a demo that
can answer nothing.

It never prints a credential: the deployment token is read from a file this process can read and is
used only as a request header.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request


class _RefuseRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args: object, **_kwargs: object) -> None:
        return None


NO_REDIRECT = urllib.request.build_opener(_RefuseRedirects())


# The two operations the served document must describe, and the method each is reached by. The demo
# exposes exactly these; a document that stopped carrying one is a demo that cannot answer, however
# alive its sockets look.
OPERATIONS = (("/v1/catalog", "get"), ("/v1/query", "post"))

# Both drawn from the shipped corpus (`examples/single-player`, `demo/Dockerfile` copies it in), so
# neither question is invented for this file: an answer and a refusal the demo already claims to
# produce. Deterministic and local - no model, no network beyond the server this container runs.
_A_REAL_QUESTION = {
    "metric": "active_subscriptions",
    "grain": "month",
    "range": {"start": "2026-01-01", "end": "2026-07-01"},
}
_AN_UNANSWERABLE_QUESTION = {
    "metric": "customer_lifetime_value",
    "grain": "month",
    "range": {"start": "2026-06-01", "end": "2026-07-01"},
}


def fetch(url: str, token: str | None = None, timeout: float = 4.0) -> bytes:
    request = urllib.request.Request(url)
    if token is not None:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def webui_session_token(port: int) -> str:
    """Sign in through the pinned no-auth flow before reading the protected registry."""
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/v1/auths/signin",
        data=json.dumps({"email": "", "password": ""}).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with NO_REDIRECT.open(request, timeout=4) as response:
            body = json.loads(response.read())
    except (urllib.error.URLError, OSError, json.JSONDecodeError):
        fail("the chat client rejected the readiness signin")
    token = body.get("token")
    if not isinstance(token, str) or not token:
        fail("the chat client signin did not return a session")
    return token


def fail(reason: str) -> None:
    # stderr, and only the reason - never a header, a token or an environment value.
    print(f"healthcheck: {reason}", file=sys.stderr)
    sys.exit(1)


def model_endpoint_is_reachable() -> None:
    """Probe the configured model's `/models` endpoint without following redirects."""
    endpoint = os.environ.get("SUTURA_DEMO_MODEL_ENDPOINT", "")
    if not endpoint:
        fail("the model endpoint is not configured")
    request = urllib.request.Request(endpoint.rstrip("/") + "/models")
    key = os.environ.get("SUTURA_DEMO_MODEL_API_KEY", "")
    if key:
        request.add_header("Authorization", f"Bearer {key}")
    try:
        with NO_REDIRECT.open(request, timeout=4) as response:
            if response.status >= 400:
                fail(f"the model endpoint answered {response.status}")
    except urllib.error.HTTPError as problem:
        if 300 <= problem.code < 400:
            fail("the model endpoint attempted a redirect")
        fail(f"the model endpoint answered {problem.code}")
    except (urllib.error.URLError, OSError) as problem:
        reason = getattr(problem, "reason", problem)
        fail(
            f"the model endpoint did not answer from the container: {type(reason).__name__}"
        )


def webui_tools_are_registered(port: int) -> None:
    """Require Open WebUI's persisted tool registry to expose the sutura server."""
    token = webui_session_token(port)
    try:
        tools = json.loads(fetch(f"http://127.0.0.1:{port}/api/v1/tools/", token))
    except (urllib.error.URLError, OSError, json.JSONDecodeError) as problem:
        fail(
            f"the chat client's tool registry did not answer: {type(problem).__name__}"
        )
    if not any(
        tool.get("id") == "server:sutura" or tool.get("name") == "server:sutura"
        for tool in tools
        if isinstance(tool, dict)
    ):
        fail("the chat client's tool registry does not list server:sutura")


def _ask(
    sutura_port: int, token: str, question: dict[str, object]
) -> tuple[int, dict[str, object]]:
    request = urllib.request.Request(
        f"http://127.0.0.1:{sutura_port}/v1/query",
        data=json.dumps(question).encode(),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    try:
        with NO_REDIRECT.open(request, timeout=4) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as problem:
        try:
            return problem.code, json.loads(problem.read())
        except json.JSONDecodeError:
            fail("a question's response body did not parse")
    except (urllib.error.URLError, OSError, json.JSONDecodeError) as problem:
        fail(
            f"a question was not answered from the container: {type(problem).__name__}"
        )


def answers_a_real_question(sutura_port: int, token: str) -> None:
    """The registration half proves the document is served; this proves it can be USED."""
    _status, body = _ask(sutura_port, token, _A_REAL_QUESTION)
    if body.get("outcome") != "answer":
        fail(f"a real question did not answer: outcome {body.get('outcome')!r}")


def refuses_an_unanswerable_question(sutura_port: int, token: str) -> None:
    """The other half: an unanswerable question must come back a REFUSAL, not silence or a 500."""
    _status, body = _ask(sutura_port, token, _AN_UNANSWERABLE_QUESTION)
    if body.get("outcome") != "refusal":
        fail(
            f"an unanswerable question was not refused: outcome {body.get('outcome')!r}"
        )


def main() -> None:
    sutura_port = int(os.environ.get("SUTURA_DEMO_SUTURA_PORT", "9000"))
    webui_port = int(os.environ.get("SUTURA_DEMO_WEBUI_PORT", "8080"))
    run_dir = os.environ.get("SUTURA_DEMO_RUN_DIR", "/run/sutura-demo")

    # The server: liveness first, so a failure names the server rather than the document.
    try:
        body = fetch(f"http://127.0.0.1:{sutura_port}/health")
    except (urllib.error.URLError, OSError) as problem:
        fail(f"the sutura server did not answer /health: {problem}")
    if json.loads(body).get("status") != "ok":
        fail("the sutura server answered /health without status ok")

    # The tool surface: the document the chat client reads, carrying both operations. This is the
    # half that separates "a process is running" from "the demo can answer a question".
    try:
        with open(os.path.join(run_dir, "token"), encoding="utf-8") as handle:
            token = handle.read().strip()
    except OSError as problem:
        fail(f"the deployment token was not readable: {problem}")
    # An EMPTY token file is not a readable credential, and a probe that reports healthy over one is
    # the defect this guard exists for: `demo/run.sh` writes this file, a generator that produced
    # nothing leaves it zero bytes, and the server then reads `access_token: ""` as ABSENT - which
    # `sutura-config` permits on a loopback, non-production bind by design. The request below would
    # otherwise succeed carrying `Bearer ` and nothing would have been authenticated.
    #
    # THE LIMIT, beside the claim: this refuses to report a demo READY while it holds no credential.
    # It does not gate the `development` posture, which deliberately serves loopback without a
    # token; this file cannot change that and does not try to.
    if not token:
        fail("the deployment token file is empty, so this demo holds no credential")
    try:
        document = json.loads(
            fetch(f"http://127.0.0.1:{sutura_port}/openapi.json", token)
        )
    except (urllib.error.URLError, OSError, json.JSONDecodeError) as problem:
        fail(
            f"the served interface description did not read back: {type(problem).__name__}"
        )
    paths = document.get("paths", {})
    declared = {
        (route, method)
        for route, operations in paths.items()
        if isinstance(operations, dict)
        for method in operations
    }
    expected = set(OPERATIONS)
    if declared != expected:
        fail(
            f"the served document operations were {sorted(declared)!r}, "
            f"expected {sorted(expected)!r}"
        )

    # Model discovery and the authenticated registry read are expensive and may reach a hosted
    # provider. Run them once per container; the server, document and client liveness checks remain
    # per-probe so a later failure still marks the container unhealthy.
    latch = os.path.join(run_dir, "external-readiness-ok")
    if not os.path.exists(latch):
        model_endpoint_is_reachable()
        webui_tools_are_registered(webui_port)
        # The registration half above proves the document is served; these two prove it answers -
        # a real question, and a genuine refusal, both against the shipped corpus. Behind the same
        # latch as the other external checks: this is a smoke test, not a per-probe repeat.
        answers_a_real_question(sutura_port, token)
        refuses_an_unanswerable_question(sutura_port, token)
        try:
            with open(latch, "x", encoding="ascii") as marker:
                marker.write("ok\n")
        except FileExistsError:
            pass

    print(
        "healthcheck: the sutura server, its two operations and the chat client are all up"
    )


if __name__ == "__main__":
    main()
