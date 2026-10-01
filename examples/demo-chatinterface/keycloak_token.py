#!/usr/bin/env python3
"""`just demo-mcp`'s issuer half: one access token from the nix Keycloak tier, and what verifies it.

Reads the realm file `nix/keycloak-tier.nix` writes, asks the realm's token endpoint for a token as
its first subject over the tier's own CA, and fetches the realm's key set. Prints four lines for
`examples/demo-chatinterface/start.sh` to capture - the issuer, the token's `https://` audience, the
token, and the key set as one line of JSON - and nothing else. A failure names its step on stderr
and never a credential, a response body or a password.

THE LIMIT: the token lives for the realm's access-token lifespan (an hour, `nix/keycloak-tier.nix`)
and nothing refreshes it, so the chat client's MCP connection stops being accepted when it expires;
running the demo again mints a new one.
"""

from __future__ import annotations

import base64
import json
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request


class _RefuseRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args: object, **_kwargs: object) -> None:
        return None


def fail(reason: str) -> None:
    print(f"demo-mcp: {reason}", file=sys.stderr)
    sys.exit(1)


def audience(token: str) -> str:
    """The `aud` a deployment declares as its resource: the claim if it is one string, else its one `https://` entry.

    Keycloak adds its own `account` audience beside the tier's mapper, so "first entry" would be an
    accident of mapper order - the harness's `audience_of` makes the same choice for the same reason.
    """
    try:
        segment = token.split(".")[1]
        claims = json.loads(
            base64.urlsafe_b64decode(segment + "=" * (-len(segment) % 4))
        )
    except (IndexError, ValueError):
        fail("the token endpoint answered something that is not a JWT")
    aud = claims.get("aud")
    if isinstance(aud, str):
        return aud
    if isinstance(aud, list):
        for entry in aud:
            if isinstance(entry, str) and entry.startswith("https://"):
                return entry
    fail("the token carries no https:// audience")


def main(realm_path: str) -> None:
    try:
        with open(realm_path, encoding="utf-8") as handle:
            realm = json.load(handle)
        issuer = realm["issuer"]
        client = realm["client"]
        subject = realm["subjects"][0]
        cafile = realm["tls_certificate_file"]
        grant = urllib.parse.urlencode(
            {
                "grant_type": "password",
                "client_id": client["id"],
                "client_secret": client["secret"],
                "username": subject["username"],
                "password": subject["password"],
            }
        ).encode()
    except (OSError, ValueError, KeyError, IndexError, TypeError):
        fail(
            f"{realm_path} is not a provisioned realm file - run just keycloak-tier start"
        )
    try:
        # The tier's own CA and nothing else. An `http://` issuer gets no TLS at all, and the server
        # refuses to start over one, so a token minted that way never verifies anywhere.
        handlers: list[urllib.request.BaseHandler] = [_RefuseRedirects()]
        if issuer.startswith("https://"):
            handlers.append(
                urllib.request.HTTPSHandler(
                    context=ssl.create_default_context(cafile=cafile)
                )
            )
        opener = urllib.request.build_opener(*handlers)
        with opener.open(
            f"{issuer}/protocol/openid-connect/token", grant, timeout=30
        ) as reply:
            token = json.loads(reply.read()).get("access_token")
        with opener.open(
            f"{issuer}/protocol/openid-connect/certs", timeout=30
        ) as reply:
            key_set = json.loads(reply.read())
    except (urllib.error.URLError, OSError, ValueError) as problem:
        fail(
            f"the realm did not mint a token and publish its key set: {type(problem).__name__}"
        )
    if not isinstance(token, str) or not token:
        fail("the token endpoint answered without an access token")
    resource = audience(token)
    print(issuer)
    print(resource)
    print(token)
    print(json.dumps(key_set, separators=(",", ":")))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ".sutura-dev/keycloak-realm.json")
