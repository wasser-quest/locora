#!/usr/bin/env python3
"""Writes the start splash animations into apps/app/ui/index.html.

Usage: python3 assets/ui/src/splash_html.py

First the exported SVGs (apps/app/ui/splash/*.svg, exported from the .sply files of
assets/ui/src/splash.py) get the runtime the app ships: `splinely-cli export` embeds
its own runtime, which is swapped for crates/locora-app-core/vendor/splinely-X.Y.Z.js,
and the version stamps say X.Y.Z. Run again, nothing changes.

Then the four animations are inlined as <template>s, so the page has them before
the wasm starts: no download, nothing to wait for. The page script clones one of them
at random (the web app mounts the .svg next to the page instead). The SVG stays as it
is, CDATA included: an <svg> in an HTML page is foreign content, where the parser
reads <![CDATA[ ... ]]> as plain text, so the runtime's `<` and `&` stay code. Without
the wrapper every `<` in it would start a tag and break the script apart.

The apps' content security policy (apps/app/src-tauri/tauri.conf.json) lets these
scripts run: Tauri adds the hashes of the page's inline scripts at build time, but not
of scripts inside a <template>, so the sha256 sources of its `script-src` are the
splash scripts' and are written here.

Only the templates, the list of scene names and those hashes are replaced; the rest of
the page (styles, tips, the page script) is edited in index.html itself.
"""
import base64, glob, hashlib, json, os, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
UI = os.path.join(ROOT, "apps", "app", "ui")
SPLASH = os.path.join(UI, "splash")
PAGE = os.path.join(UI, "index.html")
TAURI = os.path.join(ROOT, "apps", "app", "src-tauri", "tauri.conf.json")
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
    """(the <svg> element of `path`, the code of its script)."""
    text = open(path, encoding="utf-8").read()
    svg = text[text.index("<svg"): text.rindex("</svg>") + len("</svg>")]
    # One script, all of its code inside one CDATA section (see the top of this file).
    tag = re.compile(r"<script[^>]*>").search(svg)
    close = svg.find("]]></script>")
    if (not tag or not svg.startswith("<![CDATA[", tag.end()) or svg.count("]]>") != 1 or close < 0
            or "<script" in svg[:tag.start()] + svg[close:]):
        raise SystemExit(f"{path}: expected one script with all of its code in CDATA")
    assert "</template" not in svg
    return svg, svg[tag.end() + len("<![CDATA["):close]


def csp_hash(code):
    """The CSP source that allows the inline script `code` (the browser hashes the script's
    text, with its line breaks as the HTML parser leaves them)."""
    code = code.replace("\r\n", "\n").replace("\r", "\n")
    return "'sha256-" + base64.b64encode(hashlib.sha256(code.encode("utf-8")).digest()).decode() + "'"


def allow_scripts(hashes):
    """The sha256 sources of `script-src` in tauri.conf.json become `hashes`."""
    conf = open(TAURI, encoding="utf-8").read()
    line = re.compile(r'("script-src": ")([^"]*)(")')
    if len(line.findall(conf)) != 1:
        raise SystemExit(f"{TAURI}: expected one \"script-src\"")
    keep = lambda sources: [s for s in sources.split() if not s.startswith("'sha256-")]  # noqa: E731
    conf = line.sub(lambda m: m.group(1) + " ".join(keep(m.group(2)) + hashes) + m.group(3), conf)
    sources = json.loads(conf)["app"]["security"]["csp"]["script-src"].split()
    assert sources[-len(hashes):] == hashes
    open(TAURI, "w", encoding="utf-8").write(conf)


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
    inlined = [inline_svg(path) for path in files]
    templates = "\n".join(
        f'  <template id="{name}">\n{svg}\n  </template>' for name, (svg, _) in zip(names, inlined)
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
    # The scenes share one script (the runtime), so this is usually one hash.
    hashes = sorted({csp_hash(code) for _, code in inlined})
    allow_scripts(hashes)
    print("allowed", " ".join(hashes), "in", os.path.relpath(TAURI, ROOT))


if __name__ == "__main__":
    main()
