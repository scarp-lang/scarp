#!/usr/bin/env python3
"""Check the compiler's public entry points against a Scarp Docs sitemap."""

import argparse
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]
SITE = "https://scarp-lang.github.io/scarp-docs/"
ENTRY_POINTS = ("README.md", "docs/README.md", "CONTRIBUTING.md")
LINK = re.compile(r"\]\(([^\s)]+)\)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sitemap", type=Path, help="Scarp Docs build or deployed sitemap.xml")
    args = parser.parse_args()
    locations = ET.parse(args.sitemap).findall("{*}url/{*}loc")
    routes = {location.text for location in locations}
    errors = []
    checked = set()

    for filename in ENTRY_POINTS:
        file = ROOT / filename
        for target in LINK.findall(file.read_text(encoding="utf-8")):
            url = urlsplit(target)
            if target.startswith(SITE):
                checked.add(target)
                if target not in routes:
                    errors.append(f"{filename}: missing published route {target}")
            elif not url.scheme and not url.netloc and url.path:
                resolved = (file.parent / unquote(url.path)).resolve()
                if not resolved.is_relative_to(ROOT) or not resolved.exists():
                    errors.append(f"{filename}: missing local path {target}")

    for home in (SITE, SITE + "hi/"):
        if home not in checked:
            errors.append(f"missing documentation entry point {home}")
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print(f"PASS: {len(checked)} official documentation routes and local entry-point links")


if __name__ == "__main__":
    main()
