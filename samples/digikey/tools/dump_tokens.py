#!/usr/bin/env python3
"""Generate a sanitized positioned-token fixture from a DigiKey PDF.

Reads a raw PDF from samples/digikey/private/ (gitignored) with PyMuPDF and
emits one JSON array of positioned tokens in the PositionedToken schema the Rust
parser uses: {text, x, y, width, height, page}. Coordinates follow PyMuPDF's
convention (origin top-left, y increases downward, units = PDF points) — the
same convention the runtime PdfiumTextSource normalizes to.

This script contains NO personal data: the real→placeholder replacement map and
the denylist are read from gitignored files under samples/digikey/private/
(sanitize-map.txt and pii-denylist.txt), so the script itself is safe to commit.
Personal tokens are replaced with synthetic placeholders, and a final denylist
scan hard-fails if any personal string survives — so a generated fixture is
never a leak surface. Run on the authoring machine only:

    python samples/digikey/tools/dump_tokens.py \
        samples/digikey/private/SALESORDER_EMAIL100353602.pdf \
        crates/inventory-import/tests/fixtures/digikey_po_100353602.tokens.json
"""
import json
import os
import sys

import fitz  # PyMuPDF

HERE = os.path.dirname(os.path.abspath(__file__))
PRIVATE = os.path.join(HERE, "..", "private")


def load_replacements() -> dict:
    path = os.path.join(PRIVATE, "sanitize-map.txt")
    out = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            real, placeholder = line.split("=", 1)
            out[real.strip()] = placeholder.strip()
    return out


def load_denylist() -> list:
    path = os.path.join(PRIVATE, "pii-denylist.txt")
    with open(path, encoding="utf-8") as fh:
        return [
            l.strip().lower()
            for l in fh
            if l.strip() and not l.startswith("#")
        ]


def main() -> int:
    src, dest = sys.argv[1], sys.argv[2]
    replacements = load_replacements()
    denylist = load_denylist()

    doc = fitz.open(src)
    tokens = []
    for page_index in range(doc.page_count):
        for x0, y0, x1, y1, word, *_ in doc[page_index].get_text("words"):
            tokens.append(
                {
                    "text": replacements.get(word, word),
                    "x": round(x0, 2),
                    "y": round(y0, 2),
                    "width": round(x1 - x0, 2),
                    "height": round(y1 - y0, 2),
                    "page": page_index,
                }
            )

    blob = json.dumps(tokens, indent=1)
    low = blob.lower()
    leaked = [d for d in denylist if d in low]
    if leaked:
        raise SystemExit(f"PII leak — denylisted strings still present: {leaked}")

    with open(dest, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(blob)
        fh.write("\n")
    print(f"wrote {len(tokens)} tokens across {doc.page_count} pages -> {dest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
