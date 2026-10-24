# Test fixtures

Media is not committed. Run `python fixtures/fetch_fixtures.py` (needs `curl` and `ffmpeg`) to fill `fixtures/media/`, then start the test servers:

```
docker compose -f docker/docker-compose.yml up -d
python docker/init_server.py http://localhost:13378   # ABS 2.26.3
python docker/init_server.py http://localhost:13379   # ABS 2.37.2
```

Logins: `admin` / `admin-pass` (root) and `listener` / `listener-pass` (user).

| Item | Library | Source | Notes | ASIN | ISBN |
|---|---|---|---|---|---|
| The Happy Prince and Other Tales | Audiobooks | LibriVox, 5 MP3 files, 1 h 51 min | multi-file book | no | no |
| The Happy Prince Marathon | Audiobooks | LibriVox audio looped to 40 h, AAC 24 kbit/s mono | single M4B, 80 chapters of 30 min | `B0FIXTURE1` | no |
| Alice's Adventures in Wonderland | Ebooks | Project Gutenberg #11, EPUB 3 | | no | no |
| Alice PDF | Ebooks | archive.org `alicesadventur00carr` (b/w scan) | 12 MB scanned PDF | no | no |
| Test Pattern Comic | Ebooks | generated with ffmpeg `testsrc2`, 24 pages | CBZ | no | no |

No fixture has an ISBN yet. Phase 4 adds `metadata.json` overrides with ISBNs and a second, overlapping library.

ABS 2.26.3 truncates the multi-file book's title to "The Happy Prince and Other Tal" (from the ID3 album tag); 2.37.2 reads it in full.
