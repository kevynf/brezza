# /// script
# requires-python = ">=3.10"
# dependencies = ["fonttools>=4.50"]
# ///
import json
import re
import urllib.request
from pathlib import Path

from fontTools import subset

VERSION = "1.72.0"
BASE = f"https://unpkg.com/lucide-static@{VERSION}"
ROOT = Path(__file__).resolve().parent.parent
CACHE = Path.home() / ".cache" / "brezza" / f"lucide-{VERSION}"


def fetch(name: str) -> Path:
    path = CACHE / name.replace("/", "_")
    if not path.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(f"{BASE}/{name}") as response:
            path.write_bytes(response.read())
    return path


def main() -> None:
    codepoints = json.loads(fetch("font/codepoints.json").read_text(encoding="utf-8"))
    names = sorted(
        {
            name.lower().replace("_", "-")
            for path in (ROOT / "src").rglob("*.rs")
            for name in re.findall(r"icons::([A-Z0-9_]+)", path.read_text(encoding="utf-8"))
        }
    )
    missing = [name for name in names if name not in codepoints]
    if missing:
        raise SystemExit(f"unknown icons: {', '.join(missing)}")

    options = subset.Options()
    options.hinting = False
    options.desubroutinize = True
    options.name_IDs = ["*"]
    font = subset.load_font(str(fetch("font/lucide.ttf")), options)
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=[codepoints[name] for name in names])
    subsetter.subset(font)
    subset.save_font(font, str(ROOT / "assets" / "lucide.ttf"), options)
    (ROOT / "assets" / "LICENSE-lucide").write_bytes(fetch("LICENSE").read_bytes())

    lines = []
    for name in names:
        const = name.upper().replace("-", "_")
        lines.append(f'pub const {const}: &str = "\\u{{{codepoints[name]:X}}}";')
    (ROOT / "src" / "ui" / "icons.rs").write_text("\n".join(lines) + "\n", newline="\n")


if __name__ == "__main__":
    main()
