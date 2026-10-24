# 0001: License

Status: accepted

## Decision

The project is licensed under GPL-3.0-or-later.

## Reasons

- Slint's free license for desktop apps is GPL-3.0 (or its royalty-free license, which needs attribution). GPL-3.0 lets us use Slint without extra terms.
- libmpv is LGPL when built with `-Dgpl=false`. Linking LGPL code from a GPL program is allowed, and keeping the GPL option open means a GPL libmpv build would also be acceptable on Linux, where distributions ship it that way.
- Audiobookshelf itself is GPL-3.0, so contributors from that community know the terms.

## Consequences

- Third-party license texts ship with every release (see Phase 1.10).
- CBR support stays out because the unrar license is not GPL compatible.
