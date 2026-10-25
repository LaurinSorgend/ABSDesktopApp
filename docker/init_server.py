"""Initialise a fresh Audiobookshelf test server.

Creates the root admin, the test users and one library per fixture folder,
then scans them, waits for the scans and adds one collection and one series.
The Matching library gets metadata that differs between the 2.26 and the newer
server, for the work matching tests. Safe to rerun: existing users and libraries are kept.

Usage: python docker/init_server.py http://localhost:13379
"""

import json
import sys
import time
import urllib.error
import urllib.request

ADMIN = {"username": "admin", "password": "admin-pass"}
USERS = [
    {"username": "listener", "password": "listener-pass", "type": "user", "isActive": True},
    {"username": "leaver", "password": "leaver-pass", "type": "user", "isActive": True},
    {"username": "traveller", "password": "traveller-pass", "type": "user", "isActive": True},
    {"username": "reader", "password": "reader-pass", "type": "user", "isActive": True},
]
COLLECTION = "Fixture favourites"
SERIES = "Fixture series"
LIBRARIES = [
    {"name": "Audiobooks", "mediaType": "book", "folders": [{"fullPath": "/media/audiobooks"}]},
    {"name": "Ebooks", "mediaType": "book", "folders": [{"fullPath": "/media/ebooks"}]},
    {"name": "Grid", "mediaType": "book", "folders": [{"fullPath": "/media/grid"}]},
    {"name": "Matching", "mediaType": "book", "folders": [{"fullPath": "/media/matching"}]},
]

# Metadata for the Matching library by relPath: on every server, then per server
# ("old" for 2.26, "new" for later versions).
MATCHING_COMMON = {
    "Fixture Author/Shared Narration": {"narrators": ["Common Reader"]},
    "Fixture Author/Fixture Tale Abridged": {"title": "Fixture Tale", "abridged": True},
    "Fixture Author/Asin Book": {"asin": "B0MATCH001"},
}
MATCHING_VARIANT = {
    "old": {
        "Fixture Author/Two Narrators": {"narrators": ["Narrator One"]},
        "Fixture Author/Isbn Book": {"isbn": "0-261-10221-4"},
    },
    "new": {
        "Fixture Author/Two Narrators": {"narrators": ["Narrator Two"]},
        "Fixture Author/Asin Book": {"title": "Asin Book: Collector's Edition", "asin": "B0MATCH001"},
        "Fixture Author/Isbn Book": {"title": "The Isbn Book Revisited", "isbn": "978-0-261-10221-7"},
    },
}


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
    """Create the non-admin users. `leaver` is only used by logout and playback tests,
    `traveller` only by the offline session tests, `reader` only by the ebook tests."""
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


def item_count(base, token, library_id):
    """Items in a library; 0 while a fresh server answers without a total."""
    page = request(base, "GET", f"/api/libraries/{library_id}/items?limit=1", token=token)
    return page.get("total", 0) if isinstance(page, dict) else 0


def wait_for_scans(base, token, timeout=300):
    """Wait until every library has items and the counts stop changing."""
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        libraries = request(base, "GET", "/api/libraries", token=token)["libraries"]
        counts = [item_count(base, token, lib["id"]) for lib in libraries]
        if all(counts) and counts == last:
            print(f"scans done: {counts}")
            return
        last = counts
        time.sleep(3)
    sys.exit(f"scans did not finish within {timeout} s")


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


def ensure_matching_metadata(base, token):
    """Apply the per-server metadata of the Matching library."""
    version = request(base, "GET", "/status")["serverVersion"]
    variant = "old" if version.startswith("2.26.") else "new"
    libraries = request(base, "GET", "/api/libraries", token=token)["libraries"]
    library = next(lib for lib in libraries if lib["name"] == "Matching")
    items = request(base, "GET", f"/api/libraries/{library['id']}/items?minified=1", token=token)["results"]
    for item in items:
        rel = item["relPath"].replace("\\", "/")
        metadata = {**MATCHING_COMMON.get(rel, {}), **MATCHING_VARIANT[variant].get(rel, {})}
        if metadata:
            request(base, "PATCH", f"/api/items/{item['id']}/media", {"metadata": metadata}, token)
            print(f"matching metadata for {rel} ({variant})")


def main():
    base = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://localhost:13379"
    try:
        ensure_root(base)
        token = login(base)
        ensure_users(base, token)
        ensure_libraries(base, token)
        wait_for_scans(base, token)
        ensure_collection(base, token)
        ensure_series(base, token)
        ensure_matching_metadata(base, token)
    except urllib.error.HTTPError as err:
        sys.exit(f"{err.code} {err.reason}: {err.read().decode()}")


if __name__ == "__main__":
    main()
