#!/usr/bin/env python3
"""Writes the start splash animations into apps/app/ui/index.html.

Usage: python3 assets/ui/src/splash_html.py

First the exported SVGs (apps/app/ui/splash/*.svg, exported from the .sply files of
assets/ui/src/splash.py) get the runtime the app ships: `splinely-cli export` embeds
its own runtime, which is swapped for crates/locora-app-core/vendor/splinely-X.Y.Z.js,
and the version stamps say X.Y.Z. Run again, nothing changes.

Then the four animations are inlined as <template>s, so the page has them before
the wasm starts: no download, nothing to wait for. The page script clones one of them
at random (the web app mounts the .svg next to the page instead).

Only the templates and the list of scene names are replaced; the rest of the page
(styles, tips, the page script) is edited in index.html itself.
"""
import glob, os, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
UI = os.path.join(ROOT, "apps", "app", "ui")
SPLASH = os.path.join(UI, "splash")
PAGE = os.path.join(UI, "index.html")
VENDOR = os.path.join(ROOT, "crates", "locora-app-core", "vendor")


def vendored_runtime():
    """(version, code) of the Splinely runtime in crates/locora-app-core/vendor."""
    found = [(m.group(1), path) for path in glob.glob(os.path.join(VENDOR, "splinely-*.js"))
             if (m := re.fullmatch(r"splinely-(\d+\.\d+\.\d+)\.js", os.path.basename(path)))]
    if len(found) != 1:
        raise SystemExit(f"{VENDOR}: expected one splinely-X.Y.Z.js, found {len(found)}")
    version, path = found[0]
    return version, open(path, encoding="utf-8").read()


def use_runtime(svg, version, code):
    """The exported SVG with the runtime `code` in place of the one splinely-cli embedded
    (the host lines after it stay) and stamped with `version`."""
    start = svg.index("<![CDATA[", svg.index("<script")) + len("<![CDATA[")
    last = code.rstrip("\n").rsplit("\n", 1)[-1] + "\n"   # the runtime's closing line
    end = svg.index(last, start) + len(last)
    if not svg.startswith(code.split("\n", 1)[0], start):
        raise SystemExit("the exported SVG does not start its script with the Splinely runtime")
    svg = svg[:start] + code + svg[end:]
    svg, stamps = re.subn(r"<!-- Created with Splinely [\d.]+ -->", f"<!-- Created with Splinely {version} -->", svg, count=1)
    svg, generators = re.subn(r'(<splinely:generator [^>]*name="Splinely" version=")[\d.]+"', rf'\g<1>{version}"', svg, count=1)
    if stamps != 1 or generators != 1:
        raise SystemExit("the exported SVG has no Splinely version stamp")
    return svg


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
    version, code = vendored_runtime()
    for path in files:
        svg = open(path, encoding="utf-8").read()
        swapped = use_runtime(svg, version, code)
        if swapped != svg:
            open(path, "w", encoding="utf-8").write(swapped)
            print("runtime", version, "in", os.path.relpath(path, ROOT))
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
