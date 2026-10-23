#!/usr/bin/env python3
"""Writes the start splash animations into apps/app/ui/index.html.

Usage: python3 assets/ui/src/splash_html.py

The four Splinely animations (apps/app/ui/splash/*.svg, exported from the .sply files
of assets/ui/src/splash.py) are inlined as <template>s, so the page has them before
the wasm starts: no download, nothing to wait for. The embedded runtime keeps its
code; only the CDATA wrapper is removed, because script contents in HTML are plain
text. The page script clones one of them at random (the web app mounts the .svg
next to the page instead).

Only the templates and the list of scene names are replaced; the rest of the page
(styles, tips, the page script) is edited in index.html itself.
"""
import glob, os, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
UI = os.path.join(ROOT, "apps", "app", "ui")
SPLASH = os.path.join(UI, "splash")
PAGE = os.path.join(UI, "index.html")


def inline_svg(path):
    text = open(path, encoding="utf-8").read()
    svg = text[text.index("<svg"): text.rindex("</svg>") + len("</svg>")]
    # The CDATA wrapper is XML; inside an HTML script it would be a syntax error.
    svg = svg.replace("<![CDATA[", "", 1)
    svg = re.sub(r"\]\]>(\s*</script>)", r"\1", svg, count=1)
    assert "CDATA" not in svg and "</template" not in svg
    return svg


def main():
    files = sorted(glob.glob(os.path.join(SPLASH, "splash-*.svg")))
    names = [os.path.splitext(os.path.basename(f))[0] for f in files]
    templates = "\n".join(
        f'  <template id="{name}">\n{inline_svg(path)}\n  </template>'
        for name, path in zip(names, files)
    )
    page = open(PAGE, encoding="utf-8").read()
    # From the first splash template to the end of the last one.
    block = re.compile(r'  <template id="splash-.*\n  </template>', re.S)
    scenes = re.compile(r"var scenes = \[[^\]]*\];")
    if len(block.findall(page)) != 1 or len(scenes.findall(page)) != 1:
        raise SystemExit(f"{PAGE}: expected the splash templates and one `var scenes = [...]`")
    page = block.sub(lambda _: templates, page)
    page = scenes.sub(lambda _: f"var scenes = {names!r};", page)
    open(PAGE, "w", encoding="utf-8").write(page)
    print("wrote", os.path.relpath(PAGE, ROOT), "with", len(names), "splash animations")


if __name__ == "__main__":
    main()
