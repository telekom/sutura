"""The identity provider and ClickHouse token checker `nix/multi-node.nix` runs, over one ES256 key.

  jwks <public.pem> <kid>                      the key set both nodes verify against
  mint <private.pem> <kid> <sub> <aud> <scope> a caller's own access token
  idp <private.pem> <kid> <secret-file> <port> RFC 8693 exchange on 127.0.0.1, `client_secret_post`
  checker <public.pem> <port>                  ClickHouse's `http_authentication` server

A fake: one key, one client, one issuer, no expiry policy beyond `exp`. Each exchange and each
admission prints one line, which is what the test reads out of the journal.
"""

import base64
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs

import jwt
from cryptography.hazmat.primitives.serialization import (
    load_pem_private_key,
    load_pem_public_key,
)

ISSUER = "https://idp"
ACCESS_TOKEN = "urn:ietf:params:oauth:token-type:access_token"
TOKEN_EXCHANGE = "urn:ietf:params:oauth:grant-type:token-exchange"
CLICKHOUSE = "clickhouse"


def signed(key, kid, sub, aud, scope):
    now = int(time.time())
    claims = {
        "iss": ISSUER,
        "sub": sub,
        "aud": aud,
        "iat": now,
        "exp": now + 300,
        "scope": scope,
    }
    return jwt.encode(
        claims, key, algorithm="ES256", headers={"kid": kid, "typ": "at+jwt"}
    )


def verified(public, token, audience=None):
    return jwt.decode(
        token,
        public,
        algorithms=["ES256"],
        issuer=ISSUER,
        audience=audience,
        # Each VM has its own clock; a token minted on one is checked on the other.
        leeway=30,
        options={"verify_aud": audience is not None, "require": ["exp", "iss", "sub"]},
    )


def serve(port, handler):
    HTTPServer(("127.0.0.1", port), handler).serve_forever()


def idp(private_path, kid, secret_path, port):
    key = load_pem_private_key(Path(private_path).read_bytes(), None)
    public = key.public_key()
    secret = Path(secret_path).read_text().strip()

    class Exchange(BaseHTTPRequestHandler):
        def do_POST(self):
            form = {
                k: v[0]
                for k, v in parse_qs(
                    self.rfile.read(int(self.headers["Content-Length"])).decode()
                ).items()
            }
            try:
                if (
                    form.get("grant_type"),
                    form.get("client_id"),
                    form.get("client_secret"),
                ) != (TOKEN_EXCHANGE, "sutura", secret):
                    raise ValueError("not this client's token exchange")
                sub = verified(public, form["subject_token"])["sub"]
            except (ValueError, KeyError, jwt.PyJWTError) as cause:
                print(f"refuse {cause}", flush=True)
                return self.answer(400, {"error": "invalid_request"})
            audience = form["audience"]
            print(f"exchange sub={sub} audience={audience}", flush=True)
            self.answer(
                200,
                {
                    "access_token": signed(key, kid, sub, audience, "sutura:legs.run"),
                    "issued_token_type": ACCESS_TOKEN,
                    "token_type": "Bearer",
                    "expires_in": 300,
                },
            )

        def answer(self, status, body):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    serve(port, Exchange)


def checker(public_path, port):
    public = load_pem_public_key(Path(public_path).read_bytes())

    class Check(BaseHTTPRequestHandler):
        def do_GET(self):
            user = "-"
            try:
                basic = self.headers["Authorization"].removeprefix("Basic ")
                user, token = base64.b64decode(basic).decode().split(":", 1)
                admitted = verified(public, token, CLICKHOUSE)["sub"] == user
            except (ValueError, KeyError, AttributeError, jwt.PyJWTError):
                admitted = False
            print(f"{'admit' if admitted else 'deny'} {user}", flush=True)
            self.send_response(200 if admitted else 401)
            self.send_header("Content-Length", "0")
            self.end_headers()

    serve(port, Check)


def coordinate(n):
    return base64.urlsafe_b64encode(n.to_bytes(32, "big")).rstrip(b"=").decode()


def jwks(public_path, kid):
    numbers = load_pem_public_key(Path(public_path).read_bytes()).public_numbers()
    x, y = coordinate(numbers.x), coordinate(numbers.y)
    key = {
        "kty": "EC",
        "crv": "P-256",
        "x": x,
        "y": y,
        "kid": kid,
        "alg": "ES256",
        "use": "sig",
    }
    print(json.dumps({"keys": [key]}))


def mint(private_path, kid, sub, aud, scope):
    print(
        signed(
            load_pem_private_key(Path(private_path).read_bytes(), None),
            kid,
            sub,
            aud,
            scope,
        )
    )


if __name__ == "__main__":
    command, *rest = sys.argv[1:]
    if command in ("idp", "checker"):
        rest[-1] = int(rest[-1])
    {"jwks": jwks, "mint": mint, "idp": idp, "checker": checker}[command](*rest)
