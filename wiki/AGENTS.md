# Wiki Maintenance Guide

This directory is the persistent wiki for `svsim`. Treat the rest of the repository as the raw source layer unless the user explicitly asks for code or doc changes outside `wiki/`.

## Directory Contract

- `index.md`: content-oriented catalog of wiki pages. Update it whenever pages are added, removed, or renamed.
- `log.md`: append-only chronology of wiki maintenance. Add one entry per substantial ingest, query, or lint pass.
- `overview.md`: the fastest high-level orientation page.
- `status/`: verified snapshots and current-state pages.
- `architecture/`: pipeline, runtime, crate, and module explanations.
- `testing/`: corpus layout, regression strategy, and compatibility surface.
- `ports/`: imported designs and case studies.
- `roadmap/`: open edges, follow-up ideas, and capability gaps.
- `sources/`: maps from wiki pages back to raw source files.

## Working Rules

1. Read [index.md](./index.md) before creating new pages.
2. Prefer updating an existing page over creating a near-duplicate.
3. Distinguish `verified` claims from `documented` claims. If you did not run the command, label the claim as documented.
4. Keep each page centered on one question or one concept.
5. Cross-link sideways to related wiki pages and downward to raw sources.
6. When you make substantive wiki edits, update [index.md](./index.md) and append a dated entry to [log.md](./log.md).

## Writing Rules

- Use standard markdown links.
- Put raw-source links in a `## Sources` section near the bottom of a page.
- Prefer concise synthesis over long pasted summaries.
- Keep commands in fenced code blocks.
- Never invent test status, corpus counts, or support claims. Verify them or label them as historical/documented.
