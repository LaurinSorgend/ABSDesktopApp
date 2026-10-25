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

The Matching library (`media/matching`, short silent MP3s) is for the work matching tests. `docker/init_server.py` patches its metadata, partly differently on the 2.26.3 ("old") and 2.37.2 ("new") server, so the two servers overlap like two real libraries would:

| Folder | Both servers | Old server | New server | Same work across servers |
|---|---|---|---|---|
| Fixture Author/Shared Narration | narrator Common Reader | | | yes |
| Fixture Author/Two Narrators | | narrator Narrator One | narrator Narrator Two | no |
| Fixture Author/Fixture Tale | | | | yes, never with the abridged one |
| Fixture Author/Fixture Tale Abridged | title Fixture Tale, abridged, 5 instead of 12 min | | | yes |
| Other Author/Fixture Tale | | | | yes, never with Fixture Author's |
| Fixture Author/Asin Book | ASIN `B0MATCH001` | | title "Asin Book: Collector's Edition" | yes, by ASIN |
| Fixture Author/Isbn Book | | ISBN `0-261-10221-4` | ISBN `978-0-261-10221-7`, title "The Isbn Book Revisited" | yes, by ISBN |

ABS 2.26.3 truncates the multi-file book's title to "The Happy Prince and Other Tal"; 2.37.2 shows it in full.
