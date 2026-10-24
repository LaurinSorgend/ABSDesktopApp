"""Capture JSON responses from a test server into fixtures/api/<version>/.

Tokens are redacted. The files feed the offline decode tests in abs-api.

Usage: python fixtures/capture_api.py http://localhost:13379
"""

import json
import pathlib
import sys
import urllib.error
import urllib.request

ADMIN = {"username": "admin", "password": "admin-pass"}
SECRET_KEYS = {"accessToken", "refreshToken", "token"}
OUT = pathlib.Path(__file__).parent / "api"


def request(base, method, path, body=None, token=None, headers=None):
    """Send a JSON request and return the decoded response."""
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(base + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    for key, value in (headers or {}).items():
        req.add_header(key, value)
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=30) as resp:
        text = resp.read().decode()
    return json.loads(text) if text.strip().startswith(("{", "[")) else text


def redact(value):
    """Replace token strings so fixtures hold no secrets."""
    if isinstance(value, dict):
        return {k: "REDACTED" if k in SECRET_KEYS and v else redact(v) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v) for v in value]
    return value


class Capture:
    """Writes one fixture file per call."""

    def __init__(self, base):
        self.base = base
        status = request(base, "GET", "/status")
        self.dir = OUT / status["serverVersion"]
        self.dir.mkdir(parents=True, exist_ok=True)
        self.save("status", status)
        login = request(base, "POST", "/login", ADMIN, headers={"x-return-tokens": "true"})
        self.save("login", login)
        self.token = login["user"].get("accessToken") or login["user"]["token"]

    def save(self, name, value):
        path = self.dir / f"{name}.json"
        path.write_text(json.dumps(redact(value), indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        print(f"wrote {path}")

    def get(self, name, path):
        value = request(self.base, "GET", path, token=self.token)
        self.save(name, value)
        return value

    def call(self, name, method, path, body=None):
        value = request(self.base, method, path, body, token=self.token)
        self.save(name, value)
        return value


def library_by_name(libraries, name):
    return next(lib for lib in libraries["libraries"] if lib["name"] == name)


def capture_library(cap, lib):
    lib_id = lib["id"]
    cap.get("library_items", f"/api/libraries/{lib_id}/items?minified=1&limit=5&page=0&sort=media.metadata.title")
    cap.get("library_series", f"/api/libraries/{lib_id}/series?limit=5&page=0")
    cap.get("library_authors", f"/api/libraries/{lib_id}/authors")
    cap.get("library_collections", f"/api/libraries/{lib_id}/collections")
    cap.get("library_search", f"/api/libraries/{lib_id}/search?q=happy&limit=5")
    cap.get("library_personalized", f"/api/libraries/{lib_id}/personalized?limit=5")


def capture_items(cap, lib):
    items = request(cap.base, "GET", f"/api/libraries/{lib['id']}/items?minified=1", token=cap.token)["results"]
    by_title = {i["media"]["metadata"]["title"]: i for i in items}
    multi = next(i for t, i in by_title.items() if t.startswith("The Happy Prince and"))
    single = by_title["The Happy Prince Marathon"]
    cap.get("item_multi_file", f"/api/items/{multi['id']}?expanded=1")
    cap.get("item_single_file", f"/api/items/{single['id']}?expanded=1")
    return multi


def capture_session(cap, item):
    body = {
        "deviceInfo": {"deviceId": "fixture-capture", "clientName": "abs-desktop", "clientVersion": "0.0.0"},
        "mediaPlayer": "libmpv",
        "supportedMimeTypes": ["audio/mpeg", "audio/mp4"],
        "forceDirectPlay": True,
    }
    session = cap.call("play", "POST", f"/api/items/{item['id']}/play", body)
    sync = {"currentTime": 30.0, "timeListened": 10.0, "duration": session["duration"]}
    cap.call("session_sync", "POST", f"/api/session/{session['id']}/sync", sync)
    cap.call("session_close", "POST", f"/api/session/{session['id']}/close", sync)
    cap.get("item_progress", f"/api/me/progress/{item['id']}")


def capture_bookmarks(cap, item):
    path = f"/api/me/item/{item['id']}/bookmark"
    cap.call("bookmark_create", "POST", path, {"time": 42, "title": "Fixture mark"})
    cap.call("bookmark_update", "PATCH", path, {"time": 42, "title": "Renamed mark"})
    cap.call("bookmark_delete", "DELETE", f"{path}/42")


def capture_all_progress(cap):
    """`/api/me/progress` exists from 2.36 on; older servers answer 404."""
    try:
        cap.get("me_progress", "/api/me/progress")
    except urllib.error.HTTPError as err:
        if err.code != 404:
            raise
        print("no /api/me/progress on this server")


def main():
    base = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://localhost:13379"
    try:
        cap = Capture(base)
        cap.call("authorize", "POST", "/api/authorize")
        cap.get("me", "/api/me")
        libraries = cap.get("libraries", "/api/libraries")
        audiobooks = library_by_name(libraries, "Audiobooks")
        capture_library(cap, audiobooks)
        item = capture_items(cap, audiobooks)
        capture_session(cap, item)
        capture_bookmarks(cap, item)
        capture_all_progress(cap)
    except urllib.error.HTTPError as err:
        sys.exit(f"{err.code} {err.reason} for {err.url}: {err.read().decode()}")


if __name__ == "__main__":
    main()
