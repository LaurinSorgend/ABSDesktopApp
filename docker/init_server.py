"""Initialise a fresh Audiobookshelf test server.

Creates the root admin, two more users and one library per fixture folder,
then starts a scan and adds one collection and one series. Safe to rerun: existing users and libraries are kept.

Usage: python docker/init_server.py http://localhost:13379
"""

import json
import sys
import urllib.error
import urllib.request

ADMIN = {"username": "admin", "password": "admin-pass"}
USERS = [
    {"username": "listener", "password": "listener-pass", "type": "user", "isActive": True},
    {"username": "leaver", "password": "leaver-pass", "type": "user", "isActive": True},
]
COLLECTION = "Fixture favourites"
SERIES = "Fixture series"
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


def ensure_users(base, token):
    """Create the non-admin users. `leaver` is only used by logout tests."""
    users = {u["username"]: u for u in request(base, "GET", "/api/users", token=token)["users"]}
    for spec in USERS:
        existing = users.get(spec["username"])
        if not existing:
            request(base, "POST", "/api/users", spec, token)
            print(f"created user {spec['username']}")
        elif not existing["isActive"]:
            request(base, "PATCH", f"/api/users/{existing['id']}", {"isActive": True}, token)
            print(f"activated user {spec['username']}")


def ensure_libraries(base, token):
    """Create the fixture libraries and scan them."""
    existing = {lib["name"]: lib for lib in request(base, "GET", "/api/libraries", token=token)["libraries"]}
    for spec in LIBRARIES:
        lib = existing.get(spec["name"]) or request(base, "POST", "/api/libraries", spec, token)
        request(base, "POST", f"/api/libraries/{lib['id']}/scan", token=token)
        print(f"library {spec['name']} ({lib['id']}) scanning")


def ensure_collection(base, token):
    """Create one collection in the Audiobooks library once its items exist."""
    libraries = request(base, "GET", "/api/libraries", token=token)["libraries"]
    library = next(lib for lib in libraries if lib["name"] == "Audiobooks")
    collections = request(base, "GET", f"/api/libraries/{library['id']}/collections", token=token)
    if collections["results"]:
        return
    items = request(base, "GET", f"/api/libraries/{library['id']}/items?minified=1", token=token)["results"]
    if not items:
        print("no items scanned yet, rerun to create the collection")
        return
    body = {"libraryId": library["id"], "name": COLLECTION, "books": [i["id"] for i in items]}
    request(base, "POST", "/api/collections", body, token)
    print(f"created collection {COLLECTION}")


def ensure_series(base, token):
    """Put the Audiobooks items into one series, numbered by title."""
    libraries = request(base, "GET", "/api/libraries", token=token)["libraries"]
    library = next(lib for lib in libraries if lib["name"] == "Audiobooks")
    items = request(base, "GET", f"/api/libraries/{library['id']}/items?minified=1", token=token)["results"]
    items.sort(key=lambda i: i["media"]["metadata"]["title"])
    for sequence, item in enumerate(items, start=1):
        if item["media"]["metadata"].get("seriesName"):
            continue
        series = [{"name": SERIES, "sequence": str(sequence)}]
        request(base, "PATCH", f"/api/items/{item['id']}/media", {"metadata": {"series": series}}, token)
        print(f"added {item['media']['metadata']['title']} to {SERIES} #{sequence}")


def main():
    base = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://localhost:13379"
    try:
        ensure_root(base)
        token = login(base)
        ensure_users(base, token)
        ensure_libraries(base, token)
        ensure_collection(base, token)
        ensure_series(base, token)
    except urllib.error.HTTPError as err:
        sys.exit(f"{err.code} {err.reason}: {err.read().decode()}")


if __name__ == "__main__":
    main()
