"""Initialise a fresh Audiobookshelf test server.

Creates the root admin, a second user and one library per fixture folder,
then starts a scan. Safe to rerun: existing users and libraries are kept.

Usage: python docker/init_server.py http://localhost:13379
"""

import json
import sys
import urllib.error
import urllib.request

ADMIN = {"username": "admin", "password": "admin-pass"}
USER = {"username": "listener", "password": "listener-pass", "type": "user"}
LIBRARIES = [
    {"name": "Audiobooks", "mediaType": "book", "folders": [{"fullPath": "/media/audiobooks"}]},
    {"name": "Ebooks", "mediaType": "book", "folders": [{"fullPath": "/media/ebooks"}]},
    {"name": "Grid", "mediaType": "book", "folders": [{"fullPath": "/media/grid"}]},
]


def request(base, method, path, body=None, token=None):
    """Send a JSON request and return the decoded response (or raw text)."""
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(base + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=30) as resp:
        text = resp.read().decode()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def ensure_root(base):
    """Create the root user if the server is not initialised yet."""
    status = request(base, "GET", "/status")
    print(f"server {status['serverVersion']}, initialised: {status['isInit']}")
    if not status["isInit"]:
        request(base, "POST", "/init", {"newRoot": ADMIN})
        print("created root user")


def login(base):
    """Log in as admin and return the access token."""
    resp = request(base, "POST", "/login", ADMIN)
    user = resp["user"]
    return user.get("accessToken") or user["token"]


def ensure_user(base, token):
    """Create the second, non-admin user."""
    users = request(base, "GET", "/api/users", token=token)["users"]
    if any(u["username"] == USER["username"] for u in users):
        return
    request(base, "POST", "/api/users", USER, token)
    print(f"created user {USER['username']}")


def ensure_libraries(base, token):
    """Create the fixture libraries and scan them."""
    existing = {lib["name"]: lib for lib in request(base, "GET", "/api/libraries", token=token)["libraries"]}
    for spec in LIBRARIES:
        lib = existing.get(spec["name"]) or request(base, "POST", "/api/libraries", spec, token)
        request(base, "POST", f"/api/libraries/{lib['id']}/scan", token=token)
        print(f"library {spec['name']} ({lib['id']}) scanning")


def main():
    base = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://localhost:13379"
    try:
        ensure_root(base)
        token = login(base)
        ensure_user(base, token)
        ensure_libraries(base, token)
    except urllib.error.HTTPError as err:
        sys.exit(f"{err.code} {err.reason}: {err.read().decode()}")


if __name__ == "__main__":
    main()
