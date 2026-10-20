#!/usr/bin/env python3
"""Checks that the site is static: no code runs, nothing is loaded from elsewhere.

Usage: python3 apps/site/check.py   (exit 1 with a list of findings if something is not static)
"""
import os, re, sys
from html.parser import HTMLParser

HERE = os.path.dirname(os.path.abspath(__file__))
ALLOWED_FILES = {"index.html", "privacy.html", "impressum.html", "style.css", "_headers", "README.md", "check.py", "Dockerfile", "nginx.conf"}
# Only these are allowed to leave the site: the project's own repository, as a plain link.
# The forum and the browser app run as their own services at their public addresses.
ALLOWED_LINKS = ("https://github.com/wasser-quest/locora", "https://bb.locora.cloud/", "https://app.locora.cloud/")
FORBIDDEN_TAGS = {"script", "iframe", "frame", "frameset", "object", "embed", "applet", "base", "form",
                  "svg", "math", "audio", "video", "source", "track", "style", "noscript", "template"}
FORBIDDEN_SCHEMES = re.compile(r"^\s*(javascript|vbscript|data|blob|file)\s*:", re.I)
CSP_TOKEN = "default-src 'none'"


class Checker(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.findings = []

    def report(self, what):
        self.findings.append(f"index.html:{self.getpos()[0]}: {what}")

    def handle_starttag(self, tag, attrs):
        if tag in FORBIDDEN_TAGS:
            self.report(f"<{tag}> is not allowed on a static page")
        for name, value in attrs:
            name = name.lower()
            value = value or ""
            if name.startswith("on"):
                self.report(f"inline handler {name}= on <{tag}>")
            if name in ("style",):
                self.report(f"inline style= on <{tag}> (keep styles in style.css)")
            if FORBIDDEN_SCHEMES.match(value):
                self.report(f"{name}={value[:30]!r} uses a forbidden scheme")
            if tag == "meta" and name == "http-equiv" and value.lower() == "refresh":
                self.report("meta refresh is not allowed")
            if tag == "link" and name == "rel" and value.lower() != "stylesheet" and value.lower() != "":
                if "icon" not in value.lower():
                    self.report(f"<link rel={value!r}> is not allowed")
            if name in ("src", "href", "action", "data", "srcset", "poster"):
                if tag == "a" and name == "href":
                    if value.startswith("#") or value.startswith(ALLOWED_LINKS):
                        continue
                    # Pages of this site, linked by file name only.
                    if re.match(r"^[\w-]+\.html(#[\w-]+)?$", value) and os.path.exists(os.path.join(HERE, value.split("#")[0])):
                        continue
                    self.report(f"link to {value[:60]!r} is not an allowed destination")
                elif tag == "link" and name == "href" and not value.startswith(("style", "#")) and not re.match(r"^[\w.\-/]+\.css$", value):
                    self.report(f"stylesheet {value!r} must be a local .css file")
                elif tag not in ("a", "link"):
                    self.report(f"<{tag} {name}={value[:40]!r}> loads something; only text is allowed")


def check_css(path):
    findings = []
    text = open(path, encoding="utf-8").read()
    for pattern, what in ((r"@import", "@import"), (r"url\s*\(", "url()"), (r"expression\s*\(", "expression()"),
                          (r"javascript\s*:", "javascript:"), (r"<", "markup")):
        if re.search(pattern, text, re.I):
            findings.append(f"style.css: {what} is not allowed")
    return findings


def main():
    findings = []
    names = set(os.listdir(HERE))
    extra = names - ALLOWED_FILES - {"__pycache__"}
    for name in sorted(extra):
        findings.append(f"unexpected file {name!r} in the static site")
    for required in ("index.html", "privacy.html", "style.css", "_headers"):
        if required not in names:
            findings.append(f"missing {required}")
    if not findings:
        for page in ("index.html", "privacy.html"):
            checker = Checker()
            checker.feed(open(os.path.join(HERE, page), encoding="utf-8").read())
            findings += [f"{page}: {f}" for f in checker.findings]
        findings += check_css(os.path.join(HERE, "style.css"))
        headers = open(os.path.join(HERE, "_headers"), encoding="utf-8").read()
        if CSP_TOKEN not in headers or "Content-Security-Policy:" not in headers:
            findings.append("_headers has no strict Content-Security-Policy")
        for page in ("index.html", "privacy.html"):
            if CSP_TOKEN not in open(os.path.join(HERE, page), encoding="utf-8").read():
                findings.append(f"{page} has no strict Content-Security-Policy meta tag")
    if findings:
        print("The site is not static:")
        for finding in findings:
            print("  -", finding)
        return 1
    print("ok: static site, no scripts, nothing loaded from elsewhere, strict policy present")
    return 0


if __name__ == "__main__":
    sys.exit(main())
