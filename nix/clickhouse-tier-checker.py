# The ClickHouse tier's TEST-ONLY token checker - never shipped, never a production checker.
#
# ClickHouse's `http_authentication_servers` sends each sign-in as `GET` with
# `Authorization: Basic base64(user:password)`. Here the password is a token: an HS256 JWT signed
# with the tier's own key. The checker answers 200 only for a valid signature, an unexpired `exp`,
# the tier's `iss` and `aud`, and a `sub` that IS the Basic user - the binding without which one
# caller's token signs in as another. A production checker verifies the issuer's JWKS instead.
#
#   serve <port> <key-file>   answer ClickHouse's checks on 127.0.0.1:<port>
#   mint <key-file> <sub>     print a token for <sub>, valid for thirty days
import base64
import hashlib
import hmac
import http.server
import json
import sys
import time

ISSUER = "sutura-clickhouse-tier"
AUDIENCE = "clickhouse"


def b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def unb64(text):
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))


def sign(key, signing_input):
    return b64(hmac.new(key, signing_input.encode(), hashlib.sha256).digest())


def mint(key, sub):
    header = b64(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    claims = {
        "iss": ISSUER,
        "aud": AUDIENCE,
        "sub": sub,
        "exp": int(time.time()) + 30 * 24 * 3600,
    }
    signing_input = header + "." + b64(json.dumps(claims).encode())
    return signing_input + "." + sign(key, signing_input)


def admits(key, authorization):
    scheme, _, credential = authorization.partition(" ")
    if scheme != "Basic":
        return False
    user, _, token = base64.b64decode(credential).decode().partition(":")
    parts = token.split(".")
    if len(parts) != 3 or json.loads(unb64(parts[0])).get("alg") != "HS256":
        return False
    if not hmac.compare_digest(sign(key, parts[0] + "." + parts[1]), parts[2]):
        return False
    claims = json.loads(unb64(parts[1]))
    return (
        claims.get("iss") == ISSUER
        and claims.get("aud") == AUDIENCE
        and isinstance(claims.get("exp"), int)
        and claims["exp"] > time.time()
        and claims.get("sub") == user
    )


def serve(port, key):
    class Checker(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            try:
                ok = admits(key, self.headers.get("Authorization", ""))
            except (AttributeError, ValueError):
                ok = False
            self.send_response(200 if ok else 401)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def log_message(self, *_):
            pass

    http.server.HTTPServer(("127.0.0.1", port), Checker).serve_forever()


if __name__ == "__main__":
    command = sys.argv[1]
    with open(sys.argv[3] if command == "serve" else sys.argv[2], "rb") as handle:
        key = handle.read()
    if command == "serve":
        serve(int(sys.argv[2]), key)
    else:
        print(mint(key, sys.argv[3]))
