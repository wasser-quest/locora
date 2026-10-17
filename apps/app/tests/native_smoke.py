#!/usr/bin/env python3
"""Real Linux/WebKitGTK acceptance smoke; no mocked IPC or DOM injection.

Requires a running X11/AT-SPI desktop, python3-gi with Atspi, xdotool, a freshly
built app/UI, and the debug frontend server at localhost:1420. Uses its own
profile, process and window. Leaves JSON evidence/profile in --artifacts.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import struct
import zlib
import subprocess
import tempfile
import time

import gi
gi.require_version("Atspi", "2.0")
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Atspi, Gtk, Gdk, GLib


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


class Scenario:
    def __init__(self, binary, artifacts):
        self.artifacts = artifacts
        self.profile = artifacts / "profile"
        self.profile.mkdir()
        self.log = open(artifacts / "app.log", "w")
        started = time.monotonic()
        self.process = subprocess.Popen([str(binary)], env={**os.environ, "LOCORA_DATA": str(self.profile)}, stdout=self.log, stderr=self.log)
        self.steps = []
        self.clicks = 0
        self.app = self.until(lambda: next((n for n in walk(Atspi.get_desktop(0)) if n.get_role_name() == "application" and n.get_process_id() == self.process.pid), None))
        self.until(lambda: self.find("Tutorial starten", "button"))
        # Upper bound: includes AT-SPI discovery and polling of the whole tree.
        self.startup_ms = round((time.monotonic() - started) * 1000)
        windows = subprocess.check_output(["xdotool", "search", "--all", "--onlyvisible", "--pid", str(self.process.pid), "--name", "^Locora$"]).decode().split()
        if len(windows) != 1:
            self.process.terminate()
            self.process.wait(timeout=8)
            self.log.close()
            raise RuntimeError(f"Expected exactly one owned visible window, got {windows}")
        self.window = windows[0]
        self.x("windowfocus", "--sync", self.window)

    def until(self, predicate, timeout=12):
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
        raise AssertionError("Timed out waiting for native UI/database condition")

    def find(self, name, role=None, prefix=False):
        return next((n for n in walk(self.app) if (n.get_name().startswith(name) if prefix else n.get_name() == name) and (not role or n.get_role_name() == role)), None)

    def click(self, name, role="button", prefix=False):
        node = self.until(lambda: self.find(name, role, prefix))
        if not node.get_state_set().contains(Atspi.StateType.ENABLED):
            raise AssertionError(f"Disabled control: {name}")
        assert node.do_action(0), name
        self.clicks += 1

    def x(self, *args):
        subprocess.run(["xdotool", *args], check=True)

    def type_entry(self, prefix, value):
        node = self.until(lambda: self.find(prefix, "entry", True))
        self.x("windowfocus", "--sync", self.window)
        assert node.grab_focus()
        self.x("key", "--clearmodifiers", "ctrl+a")
        self.x("type", "--clearmodifiers", "--delay", "1", "--", value)

    def paste(self, node, text):
        assert node.grab_focus()
        clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
        clipboard.set_text(text, -1)
        process = subprocess.Popen(["xdotool", "key", "--clearmodifiers", "ctrl+v"])
        self.until(lambda: process.poll() is not None)
        # Serve the X11 selection while the real WebView handles the paste.
        end = time.monotonic() + .5
        while time.monotonic() < end:
            GLib.MainContext.default().iteration(False)
            time.sleep(.01)

    def drag_file(self, path, destination):
        source = Gtk.Window(title="Locora E2E drag source")
        button = Gtk.Button(label="Drag fixture into Locora")
        source.add(button)
        source.set_default_size(200, 70)
        source.move(0, 0)
        button.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [Gtk.TargetEntry.new("text/uri-list", 0, 0)], Gdk.DragAction.COPY)
        button.connect("drag-data-get", lambda _w, _ctx, selection, _info, _time: selection.set_uris([path.as_uri()]))
        source.show_all()
        for _ in range(20):
            GLib.MainContext.default().iteration(False)
            time.sleep(.01)
        screen_x, screen_y = button.get_window().get_origin()[1:]
        allocation = button.get_allocation()
        x, y = screen_x + allocation.width // 2, screen_y + allocation.height // 2
        rect = destination.get_extents(Atspi.CoordType.SCREEN)
        target_x, target_y = rect.x + rect.width // 2, rect.y + rect.height // 2
        commands = ["mousemove", str(x), str(y), "mousedown", "1", "sleep", ".2", "mousemove", str(x + 15), str(y + 15), "sleep", ".2"]
        for step in range(1, 7):
            commands += ["mousemove", str(round(x + (target_x-x)*step/6)), str(round(y + (target_y-y)*step/6)), "sleep", ".1"]
        commands += ["sleep", ".3", "mouseup", "1"]
        try:
            process = subprocess.Popen(["xdotool", *commands])
            self.until(lambda: process.poll() is not None)
            self.until(lambda: self.find("Ort setzen (Bühne anklicken)", "check box"))
        finally:
            source.destroy()

    def shot(self, name, settle=1.0):
        """Screenshot evidence; the WebView keeps rendering while we wait."""
        end = time.monotonic() + settle
        while time.monotonic() < end:
            GLib.MainContext.default().iteration(False)
            time.sleep(.02)
        root = Gdk.get_default_root_window()
        shot = Gdk.pixbuf_get_from_window(root, 0, 0, root.get_width(), root.get_height())
        if shot:
            shot.savev(str(self.artifacts / f"{name}.png"), "png", [], [])

    def sql(self, query, args=()):
        with sqlite3.connect(self.profile / "locora.db") as db:
            return db.execute(query, args).fetchall()

    def record(self, name, **values):
        self.steps.append({"check": name, "status": "passed", **values})
        print(json.dumps(self.steps[-1], ensure_ascii=False), flush=True)

    def run(self):
        self.record("startup-to-interactive", ms=self.startup_ms, budget_ms=self.startup_budget, enforced=self.startup_budget is not None)
        if self.startup_budget is not None:
            assert self.startup_ms <= self.startup_budget, f"startup {self.startup_ms} ms > {self.startup_budget} ms"
        self.click("Tutorial starten")
        self.until(lambda: self.find("▶ Route ablaufen", "button"))
        assert self.sql("SELECT count(*) FROM loci")[0][0] == 9
        self.record("tutorial-created", loci=9)

        before = self.clicks
        self.click("▶ Route ablaufen")
        self.until(lambda: self.find("Aufdecken", "button", True))
        assert self.clicks - before == 1
        self.record("journey-start-click-budget", clicks=1, budget=1)
        self.x("key", "--clearmodifiers", "space")
        self.until(lambda: self.find("Leicht", "button", True) or self.find("Gut", "button", True))
        self.x("key", "--clearmodifiers", "3")
        self.until(lambda: self.find("Weiter", "button", True))
        self.record("recall-keyboard-reveal-and-grade", clicks=0)
        # Loci 2–6 are recall, 7 is the tutorial's quiz (multiple choice).
        for _ in range(5):
            self.x("key", "--clearmodifiers", "space")
            self.until(lambda: self.find("Aufdecken", "button", True))
            self.x("key", "--clearmodifiers", "space")
            self.until(lambda: self.find("Gut", "button", True) or self.find("Leicht", "button", True))
            self.x("key", "--clearmodifiers", "3")
            self.until(lambda: self.find("Weiter", "button", True))
        clicks = self.clicks
        self.x("key", "--clearmodifiers", "space")
        options = self.until(lambda: [n for n in walk(self.app) if n.get_role_name() == "check box" and n.get_name() != "Bearbeiten"] or None)
        self.x("key", "--clearmodifiers", "1")
        self.until(lambda: options[0].get_state_set().contains(Atspi.StateType.CHECKED))
        self.x("key", "--clearmodifiers", "Return")
        self.until(lambda: self.find("Weiter", "button", True))
        assert self.clicks == clicks
        self.record("quiz-keys-1-9-and-enter", clicks=0, options=len(options))
        self.click("←")
        # Open the tutorial card without triggering its separate journey button.
        self.click("Locora lernen", prefix=True)
        self.until(lambda: self.find("Bearbeiten", "check box"))
        self.click("Bearbeiten", "check box")
        self.click("Ort 1:", prefix=True)
        self.type_entry("Inhalt ", "Native E2E Inhalt")
        self.x("key", "Tab")
        self.until(lambda: bool(self.sql("SELECT id FROM loci WHERE item = ?", ("Native E2E Inhalt",))))
        self.record("autosave-persists-content")
        locus_id, x, y = self.sql("SELECT id,x,y FROM loci WHERE item = ?", ("Native E2E Inhalt",))[0]
        marker = self.until(lambda: self.find("Ort 1:", "button", True))
        assert marker.grab_focus()
        self.x("key", "Right")
        self.until(lambda: self.sql("SELECT x,y FROM loci WHERE id=?", (locus_id,))[0] != (x,y))
        moved = self.sql("SELECT x,y FROM loci WHERE id=?", (locus_id,))[0]
        self.click("Rückgängig")
        self.until(lambda: self.sql("SELECT x,y FROM loci WHERE id=?", (locus_id,))[0] == (x,y))
        self.click("Wiederholen")
        self.until(lambda: self.sql("SELECT x,y FROM loci WHERE id=?", (locus_id,))[0] == moved)
        self.record("keyboard-move-undo-redo-persisted")

        stage = self.until(lambda: self.find("Raum – im Bearbeitungsmodus", "section", True))
        before = self.sql("SELECT count(*) FROM loci")[0][0]
        self.paste(stage, "\n".join(f"E2E Begriff {n}" for n in range(1, 11)))
        self.until(lambda: self.sql("SELECT count(*) FROM loci")[0][0] == before + 10)
        positions = self.sql("SELECT x,y FROM loci WHERE item LIKE 'E2E Begriff %'")
        assert len(positions) == len(set(positions)) == 10
        assert all(0 <= x <= 1 and 0 <= y <= 1 for x,y in positions)
        self.record("list-route-native-paste-budget", mouse_clicks=0, paste_shortcuts=1, loci=10)
        # Touch-friendly undo: the toast undoes the whole list via a backend receipt.
        # Toolbar and toast both offer it; the toast comes later in document order.
        toast = self.until(lambda: ([n for n in walk(self.app) if n.get_name() == "Rückgängig" and n.get_role_name() == "button"] + [None, None])[1])
        assert toast.do_action(0)
        self.until(lambda: self.sql("SELECT count(*) FROM loci")[0][0] == before)
        self.click("Wiederholen")
        self.until(lambda: self.sql("SELECT count(*) FROM loci")[0][0] == before + 10)
        self.record("undo-toast-and-redo-list", toast_clicks=1)
        assert stage.grab_focus()
        self.shot("tutorial-room-markers", settle=2)

        self.click("←")
        self.until(lambda: self.find("Bild/Video wählen…", "button"))
        image = self.artifacts / "Native E2E picture.png"
        def chunk(kind, payload):
            return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))
        image.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 640, 360, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress((b"\x00" + b"\x55\x66\xaa" * 640) * 360)) + chunk(b"IEND", b""))
        frame = self.until(lambda: self.find("Locora", "frame"))
        self.drag_file(image.resolve(), frame)
        self.until(lambda: bool(self.sql("SELECT id FROM palaces WHERE name=?", (image.stem,))))
        self.record("native-file-drop-creates-palace-room-and-editor", drag_gestures=1, picker_clicks=0)
        stage = self.until(lambda: self.find("Raum – im Bearbeitungsmodus", "section", True))
        rect = stage.get_extents(Atspi.CoordType.SCREEN)
        count = self.sql("SELECT count(*) FROM loci")[0][0]
        self.x("windowfocus", "--sync", self.window)
        # The click budget only counts if the stage is visible without scrolling.
        window = frame.get_extents(Atspi.CoordType.SCREEN)
        top, bottom = max(rect.y, window.y), min(rect.y + rect.height, window.y + window.height)
        assert bottom - top >= 120, f"stage mostly below the fold: {top}..{bottom}"
        self.x("mousemove", str(rect.x + rect.width//2), str((top + bottom)//2), "click", "1")
        self.until(lambda: self.sql("SELECT count(*) FROM loci")[0][0] == count + 1)
        self.until(lambda: self.find("Ort ", "entry", True))
        self.x("type", "--clearmodifiers", "--delay", "1", "--", "Nativer Ort")
        self.x("key", "Return")
        self.until(lambda: bool(self.sql("SELECT id FROM loci WHERE place=?", ("Nativer Ort",))))
        self.until(lambda: not self.find("Ort ", "entry", True))
        assert self.find("Ort setzen (Bühne anklicken)", "check box").get_state_set().contains(Atspi.StateType.CHECKED)
        self.record("place-name-enter-budget", mouse_clicks=1, enter_keys=1, ready_for_next=True)
        self.shot("dropped-palace-first-locus")

        # Video locus budget: drop a video (= palace + video room + editor), then L.
        video = self.artifacts / "Native E2E video.webm"
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=10:duration=3",
                        "-c:v", "libvpx", "-b:v", "200k", str(video)], check=True)
        self.click("←")
        self.until(lambda: self.find("Bild/Video wählen…", "button"))
        self.drag_file(video.resolve(), frame)
        self.until(lambda: bool(self.sql("SELECT r.id FROM rooms r JOIN palaces p ON p.id = r.palace_id WHERE p.name=? AND r.kind='video'", (video.stem,))))
        stage = self.until(lambda: self.find("Videoraum", "section", True))
        assert stage.grab_focus()
        clicks = self.clicks
        # Retried: L only acts once the video element has loaded metadata.
        def placed():
            self.x("key", "--clearmodifiers", "l")
            return self.sql("SELECT l.time FROM loci l JOIN rooms r ON r.id = l.room_id JOIN palaces p ON p.id = r.palace_id WHERE p.name=?", (video.stem,))
        times = self.until(lambda: (time.sleep(.5), placed())[1], timeout=20)
        assert len(times) == 1 and times[0][0] is not None and times[0][0] >= 0, times
        assert self.clicks == clicks
        self.record("video-locus-key-l", clicks=0, keys=1, time=times[0][0])
        self.shot("video-locus")

        # Video timeline navigation: a second locus at a later playhead gives two
        # chronological buttons; the first pauses, seeks and selects its locus.
        timeline = lambda: [n for n in walk(self.app) if n.get_role_name() == "button" and n.get_name() and " s · " in n.get_name()]
        self.until(lambda: len(timeline()) == 1)
        # Play for a moment so the second playhead differs, then L again.
        stage = self.until(lambda: self.find("Videoraum", "section", True))
        assert stage.grab_focus()
        time.sleep(2)
        self.x("key", "--clearmodifiers", "l")
        second = self.until(lambda: self.sql("SELECT count(*) FROM loci l JOIN rooms r ON r.id = l.room_id JOIN palaces p ON p.id = r.palace_id WHERE p.name=?", (video.stem,))[0][0] == 2 or None)
        assert second
        buttons = self.until(lambda: timeline() if len(timeline()) == 2 else None)
        labels = [b.get_name() for b in buttons]
        assert labels == sorted(labels), labels
        # Clicking the earlier button seeks back, keeps both loci and no error appears.
        buttons[0].do_action(0)
        time.sleep(1)
        assert self.sql("SELECT count(*) FROM loci l JOIN rooms r ON r.id = l.room_id JOIN palaces p ON p.id = r.palace_id WHERE p.name=?", (video.stem,))[0][0] == 2
        assert not any((n.get_name() or "").strip() and "konnte nicht" in n.get_name() for n in walk(self.app))
        self.shot("video-timeline")
        self.record("video-timeline-navigation", buttons=2, chronological=True)

        # Video journey: play muted to each locus, hold there, keyboard reveal/grade.
        self.click("←")
        self.until(lambda: self.find("Bild/Video wählen…", "button"))
        self.click("Native E2E video", prefix=True)
        self.until(lambda: self.find("▶ Route ablaufen", "button"))
        clicks = self.clicks
        self.click("▶ Route ablaufen")
        self.until(lambda: self.find("Aufdecken", "button", True) or self.find("Weiter", "button", True) or self.find("Zur Übersicht", "button"))
        self.record("video-journey-starts-holding-at-locus", clicks=1, budget=1)
        # Both loci: reveal (space), grade (3), advance (space).
        for _ in range(2):
            if self.find("Zur Übersicht", "button") and any(n.get_role_name() == "paragraph" and "gewusst" in (n.get_name() or "") for n in walk(self.app)):
                break
            # The video must reach the locus before answers unlock (video_ready).
            self.until(lambda: (lambda b: b and b.get_state_set().contains(Atspi.StateType.ENABLED))(self.find("Aufdecken", "button", True)))
            self.x("key", "--clearmodifiers", "space")
            self.until(lambda: self.find("Gut", "button", True) or self.find("Leicht", "button", True))
            self.x("key", "--clearmodifiers", "3")
            self.until(lambda: self.find("Weiter", "button", True) or self.find("Zur Übersicht", "button"))
            self.x("key", "--clearmodifiers", "space")
        self.until(lambda: self.find("Zur Übersicht", "button") and self.find("Geschafft", "image"))
        assert self.clicks == clicks + 1
        self.record("video-journey-keyboard-through-loci", extra_clicks=0)
        self.shot("video-journey-summary")

        # Reverse direction: same journey, prompt and answer swap.
        self.click("Zur Übersicht")
        self.until(lambda: self.find("Bild/Video wählen…", "button"))
        self.click("Native E2E video", prefix=True)
        self.until(lambda: self.find("Rückwärts (Inhalt → Ort)", "check box"))
        self.click("Rückwärts (Inhalt → Ort)", "check box")
        self.click("▶ Route ablaufen")
        # The video must reach the locus before answers unlock (video_ready).
        self.until(lambda: (lambda b: b and b.get_state_set().contains(Atspi.StateType.ENABLED))(self.find("Aufdecken", "button", True)))
        self.x("key", "--clearmodifiers", "space")
        self.until(lambda: self.find("Gut", "button", True) or self.find("Leicht", "button", True))
        self.x("key", "--clearmodifiers", "3")
        self.until(lambda: self.find("Weiter", "button", True) or self.find("Zur Übersicht", "button"))
        self.x("key", "--clearmodifiers", "space")
        self.until(lambda: (lambda b: b and b.get_state_set().contains(Atspi.StateType.ENABLED))(self.find("Aufdecken", "button", True)))
        self.x("key", "--clearmodifiers", "space")
        self.until(lambda: self.find("Gut", "button", True) or self.find("Leicht", "button", True))
        self.x("key", "--clearmodifiers", "3")
        self.until(lambda: self.find("Weiter", "button", True) or self.find("Zur Übersicht", "button"))
        self.x("key", "--clearmodifiers", "space")
        self.until(lambda: self.find("Zur Übersicht", "button") and self.find("Geschafft", "image"))
        self.record("video-journey-reverse-direction")
        self.shot("video-journey-reverse-summary")

        # Persistence is checked from SQLite, not merely optimistic UI text.
        self.record("no-runtime-start-error", checked_views="welcome, tutorial room, first recall")
        assert not any("Animation konnte nicht" in n.get_name() for n in walk(self.app))

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.log.close()
        (self.artifacts / "results.json").write_text(json.dumps({"profile": str(self.profile), "checks": self.steps, "scope": "Actual Tauri/WebKitGTK using native accessibility and X11 input; not all Phase2 acceptance checks"}, ensure_ascii=False, indent=2))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("apps/app/target/debug/locora-app"))
    parser.add_argument("--artifacts", type=Path)
    # PLAN target is 300 ms for a release build on real hardware; CI runs a
    # debug build under Xvfb, so the budget is opt-in.
    parser.add_argument("--startup-budget-ms", type=int)
    args = parser.parse_args()
    artifacts = args.artifacts or Path(tempfile.mkdtemp(prefix="locora-native-e2e-"))
    artifacts.mkdir(parents=True, exist_ok=True)
    scenario = None
    try:
        scenario = Scenario.__new__(Scenario)
        scenario.startup_budget = args.startup_budget_ms
        scenario.__init__(args.binary.resolve(), artifacts)
        scenario.run()
    except Exception as error:
        if scenario and hasattr(scenario, "steps"):
            scenario.steps.append({"status": "failed", "error": str(error)})
        root = Gdk.get_default_root_window()
        shot = Gdk.pixbuf_get_from_window(root, 0, 0, root.get_width(), root.get_height())
        if shot:
            shot.savev(str(artifacts / "failure.png"), "png", [], [])
        raise
    finally:
        if scenario and hasattr(scenario, "process") and not scenario.log.closed:
            scenario.close()
        print(f"Artifacts: {artifacts}")


if __name__ == "__main__":
    main()
