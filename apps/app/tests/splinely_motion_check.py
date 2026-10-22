#!/usr/bin/env python3
"""Native release animation check (AT-SPI + screenshots, no injected app API).

Use run_smoke.sh --motion after building the app. Each revealed tutorial picture is
cropped separately, so moving markers/backgrounds cannot hide a static picture.
Screenshots and measurements are evidence of rendering, not runtime internals.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import gi

gi.require_version("Atspi", "2.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Atspi, Gdk, GLib
from PIL import Image, ImageChops


def walk(node, depth=0):
    if node is None:
        return
    try:
        yield node
        if depth < 24:
            for index in range(node.get_child_count()):
                yield from walk(node.get_child_at_index(index), depth + 1)
    except GLib.Error:
        return


class MotionScenario:
    def __init__(self, binary, artifacts, reduced):
        self.binary = binary
        self.artifacts = artifacts
        self.reduced = reduced
        self.process = None
        self.app = None
        self.log = None
        self.checks = []

    def until(self, predicate, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.process.poll() is not None:
                raise RuntimeError(f"App exited: {self.process.returncode}")
            while GLib.MainContext.default().pending():
                GLib.MainContext.default().iteration(False)
            result = predicate()
            if result:
                return result
            time.sleep(.05)
        raise AssertionError("Timed out waiting for native UI condition")

    def find(self, name, role="button", prefix=False):
        return next((n for n in walk(self.app)
                     if (n.get_name().startswith(name) if prefix else n.get_name() == name)
                     and (not role or n.get_role_name() == role)), None)

    def click(self, name, role="button", prefix=False):
        node = self.until(lambda: self.find(name, role, prefix))
        assert node.get_state_set().contains(Atspi.StateType.ENABLED), name
        assert node.do_action(0), name

    def key(self, value):
        subprocess.run(["xdotool", "key", "--clearmodifiers", value], check=True)

    def wait(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            GLib.MainContext.default().iteration(False)
            time.sleep(.02)

    def shot(self, name):
        root = Gdk.get_default_root_window()
        shot = Gdk.pixbuf_get_from_window(root, 0, 0, root.get_width(), root.get_height())
        path = self.artifacts / f"{name}.png"
        shot.savev(str(path), "png", [], [])
        return path

    def picture_box(self, welcome=False):
        boxes = set()
        for node in walk(self.app):
            if node.get_name() != "Animation":
                continue
            try:
                rect = node.get_extents(Atspi.CoordType.SCREEN)
            except GLib.Error:
                continue
            # Markers are 56px, feedback art is 80px. Ignore both. On a
            # journey the locus picture is smaller than the room backdrop.
            if rect.width >= 100 and rect.height >= 100:
                boxes.add((rect.x, rect.y, rect.x + rect.width, rect.y + rect.height))
        if not boxes:
            return None
        return (max if welcome else min)(boxes, key=lambda b: (b[2]-b[0]) * (b[3]-b[1]))

    def record(self, name, **values):
        entry = {"check": name, **values}
        self.checks.append(entry)
        print(json.dumps(entry, ensure_ascii=False), flush=True)

    def motion(self, name, welcome=False):
        self.until(lambda: self.picture_box(welcome))
        self.wait(1.3)
        box = self.picture_box(welcome)
        first = self.shot(f"{name}-a")
        with Image.open(first) as full_a:
            assert 0 <= box[0] < box[2] <= full_a.width, box
            assert 0 <= box[1] < box[3] <= full_a.height, box
            a = full_a.crop(box).convert("RGB")
        a.save(self.artifacts / f"{name}-crop-a.png")
        # Authored sequences hold their final pose for ~1s. Two snapshots
        # inside that hold are not evidence of a broken runtime. Sample a
        # whole sequence (longest pulse/sequence period: 3.6s).
        samples = []
        for sample in range(5):
            self.wait(.79)
            second = self.shot(f"{name}-{sample}")
            with Image.open(second) as full_b:
                b = full_b.crop(box).convert("RGB")
            b.save(self.artifacts / f"{name}-crop-{sample}.png")
            diff = ImageChops.difference(a, b).convert("L")
            samples.append(sum(value > 12 for value in diff.getdata()))
        changed = max(samples)
        self.record(name, changed_pixels=changed, samples=samples, pixels=a.width*a.height,
                    box=box, reduced_motion=self.reduced)
        if self.reduced:
            assert changed <= 10, f"{name}: animation with reduced motion ({changed} pixels)"
        else:
            assert changed > 10, f"{name}: no visible motion ({changed} pixels)"
        assert not any("konnte nicht geladen werden" in n.get_name() for n in walk(self.app))

    def run(self):
        profile = self.artifacts / "profile"
        profile.mkdir()
        env = {**os.environ, "LOCORA_DATA": str(profile)}
        if self.reduced:
            config = self.artifacts / "gtk-config" / "gtk-3.0"
            config.mkdir(parents=True)
            (config / "settings.ini").write_text("[Settings]\ngtk-enable-animations=false\n")
            env["XDG_CONFIG_HOME"] = str(config.parent)
        self.log = open(self.artifacts / "app.log", "w")
        self.process = subprocess.Popen([str(self.binary)], env=env, stdout=self.log, stderr=self.log)
        self.app = self.until(lambda: next((n for n in walk(Atspi.get_desktop(0))
                                           if n.get_role_name() == "application"
                                           and n.get_process_id() == self.process.pid), None))
        self.until(lambda: self.find("App-Tour starten"))
        window = subprocess.check_output(["xdotool", "search", "--all", "--onlyvisible",
                                          "--pid", str(self.process.pid), "--name", "^Locora$"], text=True).splitlines()[0]
        subprocess.run(["xdotool", "windowfocus", "--sync", window], check=True)
        subprocess.run(["xdotool", "windowsize", window, "1280", "1320"], check=True)
        subprocess.run(["xdotool", "mousemove", "1500", "1350"], check=True)
        self.motion("welcome-simo", welcome=True)
        # The app tour sets up the tutorial palace; close it and open the palace.
        self.click("App-Tour starten")
        # While the lessons of „Mnemotechnik lernen“ are open, the app asks first.
        self.click(self.until(lambda: self.find("Ich kenne das schon – Tour starten")).get_name())
        self.until(lambda: self.find("Schließen"))
        self.click("Schließen")
        self.click(self.until(lambda: self.find("Locora lernen", prefix=True)).get_name())
        self.click("▶ Route ablaufen")
        for index in range(1, 10):
            self.until(lambda: self.find("Aufdecken", prefix=True) or self.find("Prüfen"))
            if self.find("Aufdecken (Leertaste)"):
                self.key("space")
                self.until(lambda: self.find("Gut", prefix=True))
                self.motion(f"locus-{index}")
                self.key("3")
            elif self.find("Ehrlich, von 1 bis 4", "check box"):
                self.click("Ehrlich, von 1 bis 4", "check box")
                self.click("Prüfen")
                self.until(lambda: self.find("Weiter", prefix=True))
                self.motion(f"locus-{index}")
            elif self.find("Prüfen"):
                entry = self.until(lambda: next((n for n in walk(self.app)
                                                if n.get_role_name() == "entry"), None))
                assert entry.grab_focus()
                subprocess.run(["xdotool", "type", "--clearmodifiers", "--delay", "1", "--", "new"], check=True)
                self.click("Prüfen")
                self.until(lambda: self.find("Weiter", prefix=True))
                self.motion(f"locus-{index}")
            else:
                raise AssertionError(f"Unexpected question at locus {index}")
            self.click("Weiter", prefix=True)
        self.until(lambda: self.find("Zur Übersicht"))
        self.record("all-loci-visited", loci=9)

    def close(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        if self.log:
            self.log.close()
        (self.artifacts / "results.json").write_text(json.dumps({"checks": self.checks}, ensure_ascii=False, indent=2))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("apps/app/target/release/locora-app"))
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--reduced-motion", action="store_true")
    args = parser.parse_args()
    artifacts = args.artifacts or Path(tempfile.mkdtemp(prefix="locora-motion-"))
    artifacts.mkdir(parents=True, exist_ok=True)
    scenario = MotionScenario(args.binary.resolve(), artifacts, args.reduced_motion)
    try:
        scenario.run()
    except Exception as error:
        scenario.record("failed", error=str(error))
        try:
            scenario.shot("failure")
        except Exception as diagnostic_error:
            print(f"Failure screenshot failed: {diagnostic_error}", file=sys.stderr)
        try:
            (artifacts / "accessibility.txt").write_text("\n".join(
                f"{n.get_role_name()}: {n.get_name()}" for n in walk(scenario.app)))
        except Exception as diagnostic_error:
            print(f"Accessibility dump failed: {diagnostic_error}", file=sys.stderr)
        raise
    finally:
        scenario.close()
        print(f"Artifacts: {artifacts}")


if __name__ == "__main__":
    main()
