# Editing and building the specifications

The book uses mdBook, as do the Tor specifications. Chapter order and independent
El Tor numbering come from `SUMMARY.md`; do not put positional numbers in new
filenames or use them as protocol identifiers. Keep stable descriptive anchors.

Architecture pages explain purpose, roles, flow and tradeoffs. Technical chapters
own exact bytes, algorithms, state and validation requirements. Historical notes
must be visibly nonnormative. Update the implementation TODO and record a
privacy/timing assessment for every requirement change, as required by AGENTS.md.

From the repository root, with Python 3 and mdBook installed (validated with mdBook 0.4.43):

```sh
mdbook build
mdbook serve --hostname 127.0.0.1
```

Output is local under `target/spec-book`. Building or serving does not publish.
Keep Git operations local and preserve the workspace identity and push protections.
The book configuration deliberately contains no repository-edit or publishing link.

A documentation check must cover chapter coverage, local links and anchors,
retained legacy links, moved requirements, code-block integrity and rendered
navigation. Protocol checks remain separately tracked in the validation chapters.

The local link preprocessor renders the root TODO in the book without copying
its maintained source. Run `python3 spec/tools/check_docs.py` before building.
Run `python3 spec/tools/check_book.py` after `mdbook build` to validate all rendered
local links, images and anchors, including the generated backlog. The historical
service-flow image is retained locally for an offline book preview.
