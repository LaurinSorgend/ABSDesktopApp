# Test fixtures

Media is not committed. Run `python fixtures/fetch_fixtures.py` (needs `ffmpeg`) to generate `fixtures/media/`, then start the test servers:

```
docker compose -f docker/docker-compose.yml up -d
python docker/init_server.py http://localhost:13378   # ABS 2.26.3
python docker/init_server.py http://localhost:13379   # ABS 2.37.2
```

Logins: `admin` / `admin-pass` (root) and `listener` / `listener-pass` (user).

By default every file is generated, so CI downloads nothing. The audiobooks are pink noise with the names, lengths, bitrate and tags of the LibriVox originals; the EPUB and PDF hold filler text. `--real` (needs `curl`) downloads the public domain originals instead, for spikes and manual testing. The script skips files that exist, so delete `fixtures/media/` to switch between the two.

| Item | Library | Source | Notes | ASIN | ISBN |
|---|---|---|---|---|---|
| The Happy Prince and Other Tales | Audiobooks | 5 MP3 files, 1 h 51 min, 64 kbit/s mono; `--real`: LibriVox | multi-file book | no | no |
| The Happy Prince Marathon | Audiobooks | the multi-file book looped to 40 h, AAC 24 kbit/s mono | single M4B, 80 chapters of 30 min | `B0FIXTURE1` | no |
| Alice's Adventures in Wonderland | Ebooks | EPUB 3, 12 chapters; `--real`: Project Gutenberg #11 | | no | no |
| Alice PDF | Ebooks | 20 text pages; `--real`: archive.org `alicesadventur00carr` (12 MB b/w scan) | | no | no |
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

With the LibriVox files ABS 2.26.3 truncates the multi-file book's title to "The Happy Prince and Other Tal" (their album tag is ID3v1 only); 2.37.2 shows it in full. Tests match the title by its start.
