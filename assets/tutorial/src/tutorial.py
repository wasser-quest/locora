#!/usr/bin/env python3
"""Builds the graphics of the tutorial palace "Locora lernen" with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/tutorial/src/tutorial.py [OUTPUT_DIR]

Style: assets/STIL.md (no outlines, cel shading, light from the top left,
glowing loci in lime). Each file is a small animated scene: three room
backdrops (512 x 384) and the locus pictures (384 x 384).
"""
import json, math, os, re, subprocess, sys

CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")
OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))

INDIGO, INDIGO_SH, INDIGO_HI, INDIGO_DEEP = "#3A3D92", "#25276A", "#5258BC", "#1A1B4C"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"
BROWN, BROWN_SH, BROWN_HI = "#6E4128", "#4A2A18", "#8A5536"
GREEN, GREEN_SH, GREEN_HI = "#3E7A4A", "#2B5A36", "#5A9A62"
LIME, TRIM, GLOW_CORE = "#DDF76A", "#C9E85A", "#FFFFF0"
RED, AMBER = "#C9455F", "#E9C06A"
INK = "#23245E"


def ellipse(rx, ry):
    k = 0.5523
    return (f"M{-rx:g} 0 C {-rx:g} {-ry*k:.2f} {-rx*k:.2f} {-ry:g} 0 {-ry:g} C {rx*k:.2f} {-ry:g} {rx:g} {-ry*k:.2f} {rx:g} 0 "
            f"C {rx:g} {ry*k:.2f} {rx*k:.2f} {ry:g} 0 {ry:g} C {-rx*k:.2f} {ry:g} {-rx:g} {ry*k:.2f} {-rx:g} 0 Z")


def rounded(w, h, r):
    return (f"M{r:g} 0 L{w-r:g} 0 Q{w:g} 0 {w:g} {r:g} L{w:g} {h-r:g} Q{w:g} {h:g} {w-r:g} {h:g} "
            f"L{r:g} {h:g} Q0 {h:g} 0 {h-r:g} L0 {r:g} Q0 0 {r:g} 0 Z")


def num(v):
    return f"{round(v, 3) + 0.0:g}"


def map_path(d, fn):
    """Apply fn(x, y) -> (x, y) to every point of an absolute M/L/Q/C/Z path."""
    out, xy = [], []
    for tok in re.findall(r"[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", d):
        if tok.isalpha():
            if tok not in "MLQCZ":
                sys.exit(f"map_path: only absolute M/L/Q/C/Z paths, found {tok!r}")
            out.append(tok)
            continue
        xy.append(float(tok))
        if len(xy) == 2:
            x, y = (num(v) for v in fn(*xy))
            out[-1] += f"{x} {y}" if out[-1].isalpha() else f" {x} {y}"
            xy = []
    return " ".join(out)


# Splinely's easings (splinely_cli clips.rs) and their curve (splinely_document animation.rs),
# so a helper knows where a track is between two of its keys.
EASINGS = {"linear": None, "ease": (0.25, 0.1, 0.25, 1), "ease-in": (0.42, 0, 1, 1),
           "ease-out": (0, 0, 0.58, 1), "ease-in-out": (0.42, 0, 0.58, 1)}


def eased(easing, t):
    curve = EASINGS[easing]
    if curve is None:
        return t
    x1, y1, x2, y2 = curve
    bez = lambda s, a, b: 3 * (1 - s) ** 2 * s * a + 3 * (1 - s) * s * s * b + s ** 3  # noqa: E731
    lo, hi = 0.0, 1.0
    for _ in range(60):
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if bez(mid, x1, x2) < t else (lo, mid)
    return bez((lo + hi) / 2, y1, y2)


def sample(keys, time):
    """Value of a track [(time, value, easing to the next key)] at `time`."""
    if time <= keys[0][0]:
        return keys[0][1]
    for (t0, v0, e), (t1, v1, _) in zip(keys, keys[1:]):
        if time < t1:
            return v0 + (v1 - v0) * eased(e, (time - t0) / (t1 - t0))
    return keys[-1][1]


def rgb(colour):
    """#RRGGBB -> (r, g, b) in 0..255."""
    return tuple(int(colour[i:i + 2], 16) for i in (1, 3, 5))


def mix(a, b, w):
    """The colour between the #RRGGBB colours a (w = 0) and b (w = 1)."""
    return "#" + "".join(f"{round(x + (y - x) * w):02X}" for x, y in zip(rgb(a), rgb(b)))


class Scene:
    """One .sply file built step by step through splinely-cli."""

    def __init__(self, name, width, height, title):
        self.file = os.path.join(OUT, f"{name}.sply")
        if os.path.exists(self.file):
            os.remove(self.file)
        self.steps = []
        self.cli("new", "--output", self.file, "--width", str(width), "--height", str(height), "--name", title)

    def cli(self, *args):
        out = subprocess.run([CLI, *args], capture_output=True, text=True)
        if out.returncode != 0:
            sys.exit(f"splinely-cli {' '.join(args)}\n{out.stdout}{out.stderr}")
        return json.loads(out.stdout) if out.stdout.strip() else {}

    def add(self, kind, id, **o):
        args = ["node", "add", self.file, "--kind", kind, "--id", id]
        for k, v in o.items():
            args += [f"--{k.replace('_', '-')}", str(v)]
        self.cli(*args)
        return id

    def update(self, id, **o):
        args = ["node", "update", self.file, id]
        for k, v in o.items():
            args += [f"--{k.replace('_', '-')}", str(v)]
        self.cli(*args)

    def crescent(self, base, dx, dy, fill, opacity=None):
        """Hard cel shadow/light: `base` minus a copy shifted by (dx, dy)."""
        a = self.cli("node", "duplicate", self.file, "--nodes", base, "--dx", "0", "--dy", "0")["created"][0]
        b = self.cli("node", "duplicate", self.file, "--nodes", base, "--dx", str(dx), "--dy", str(dy))["created"][0]
        self.cli("arrange", "order", self.file, "--nodes", b, "--to", "front")
        res = self.cli("arrange", "boolean", self.file, "--op", "difference", "--nodes", f"{a},{b}")["id"]
        self.update(res, fill=fill, **({"opacity": opacity} if opacity is not None else {}))
        return res

    def pin(self, id, px, py):
        """Make the scene point (px, py) the local origin of `id` and bake its rotation and scale
        into the geometry: it looks the same, but turns and scales around (px, py)."""
        node = next(n for n in self.cli("cat", self.file)["nodes"] if n["id"] == id)
        k, t = node["kind"], node["transform"]
        c, s = math.cos(math.radians(t["rotation"])), math.sin(math.radians(t["rotation"]))
        sx, sy = t["scaleX"], t["scaleY"]

        def around(x, y):
            x, y = x * sx, y * sy
            return t["x"] + x * c - y * s - px, t["y"] + x * s + y * c - py

        o = {"tx": num(px), "ty": num(py), "rotation": 0, "scale_x": 1, "scale_y": 1}
        if k["type"] == "path":
            o["d"] = map_path(k["d"], around)
        elif k["type"] == "circle" and abs(sx - sy) < 1e-9:
            o["x"], o["y"] = (num(v) for v in around(k["cx"], k["cy"]))
            o["radius"] = num(k["radius"] * abs(sx))
        elif k["type"] in ("rect", "text") and t["rotation"] == 0 and sx == sy == 1:
            o["x"], o["y"] = (num(v) for v in around(k["x"], k["y"]))
        else:
            sys.exit(f"pin: cannot move the origin of {id} ({k['type']}, scale {sx} x {sy})")
        self.update(id, **o)

    def center(self, id):
        """Pin `id` at the centre of its bounds."""
        x, y, w, h = next(n for n in self.cli("node", "list", self.file)["nodes"] if n["id"] == id)["bounds"]
        self.pin(id, x + w / 2, y + h / 2)

    def body(self, name, parts, pivot, rims=None):
        """The parts of one moving object (see `Body`)."""
        return Body(self, name, parts, pivot, rims)

    def glow(self, id, x, y, r, strength=0.8):
        return self.add("circle", id, x=x, y=y, radius=r, pivot="center",
                        gradient=f"radial rgba(230,255,120,{strength})@0 rgba(230,255,120,0)@1")

    def light(self, id, x, y, r):
        return self.add("circle", id, x=x, y=y, radius=r, pivot="center",
                        gradient=f"radial {GLOW_CORE}@0 {LIME}@0.6 #A8CF2E@1")

    def pulse(self, clip, ids, low=0.45, period=2.4):
        for i, n in enumerate(ids):
            extra = " --autoplay --loop" if i == 0 else ""
            self.steps += [f"keyframe --clip {clip}_anim --target {n} --property opacity --time {period/2:g} --value {low} --easing ease-in-out{extra}",
                           f"keyframe --clip {clip}_anim --target {n} --property opacity --time {period:g} --value 1 --easing ease-in-out"]

    def sequence(self, clip, groups, step=0.5, hold=2.4, cycle=3.0, low=0.3):
        """Light up groups of nodes one after another (1 → 2 → 3), then fade together."""
        first = True
        for i, ids in enumerate(groups):
            on = round(0.35 + i * step, 2)
            for n in ids:
                extra = " --autoplay --loop" if first else ""
                first = False
                self.steps += [f"keyframe --clip {clip}_anim --target {n} --property opacity --time 0.01 --value {low}{extra}",
                               f"keyframe --clip {clip}_anim --target {n} --property opacity --time {on} --value 1 --easing ease-out",
                               f"keyframe --clip {clip}_anim --target {n} --property opacity --time {hold} --value 1",
                               f"keyframe --clip {clip}_anim --target {n} --property opacity --time {cycle} --value {low} --easing ease-in"]

    def finish(self, description):
        self.steps.append(f'a11y --description "{description}"')
        tmp = self.file + ".steps"
        with open(tmp, "w") as f:
            f.write("\n".join(self.steps) + "\n")
        try:
            self.cli("batch", self.file, "--steps", tmp)
        finally:
            os.remove(tmp)
        print("built", os.path.relpath(self.file))


class Body:
    """One moving object drawn as several shapes: its base, the cel shadow and light edge
    (`crescent` makes them as separate nodes), shines and its halo.

    Every part gets the same pivot (`Scene.pin`) and every motion goes to all parts at once, so
    no part stays behind when the object moves (assets/STIL.md, 9). A lift moves the group of
    the parts, which Splinely moves as a whole (an offset from where it was drawn); scale and
    rotation, which a Splinely group cannot take, are keyed on every part with the same values
    around the shared pivot. A ground shadow (`ground`) follows the same poses: smaller and
    lighter the higher the object is, wider when it squashes on landing.

    The light stays at the top left while the object turns: `rims` maps each cel rim to
    (its tone, the tone it takes when turned half round), e.g. the shadow crescent to
    (shadow, light) and the light edge to (light, shadow); in between the tones blend.
    """

    REST = {"lift": 0, "sx": 1, "sy": 1, "turn": 0}
    PROPERTY = {"lift": "y", "sx": "scaleX", "sy": "scaleY", "turn": "rotation"}

    def __init__(self, scene, name, parts, pivot, rims=None):
        self.scene, self.name, self.parts, self.pivot = scene, name, list(parts), pivot
        self.rims = dict(rims or {})
        self.group = None
        self.shadow = None
        for p in self.parts:
            scene.pin(p, *pivot)

    def ground(self, shadow, opacity, highest, smallest=0.55, faintest=0.35):
        """`shadow` lies on the ground under the object: at rest it has `opacity`; at the height
        `highest` it is `smallest` times as big and `faintest` times as dark."""
        self.scene.center(shadow)
        self.shadow = (shadow, opacity, highest, smallest, faintest)

    def states(self, prop, value):
        """`on --state` overrides that change every part the same way (an eye closing)."""
        return [f"--state {p}.{prop}={num(value)}" for p in self.parts]

    def animate(self, clip, poses, autoplay=True, loop=True):
        """`poses`: [(time, easing to the next pose, {channel: value})] with the channels `lift`
        (px up from rest), `sx`, `sy` (scale factors) and `turn` (degrees). A channel is keyed at
        the poses that name it; if the first is later than 0 s, it starts from rest."""
        s = self.scene
        poses = sorted(poses, key=lambda p: p[0])
        s.steps.append(f"clip add --id {clip} --duration {num(poses[-1][0])}")
        tracks = {}
        for t, easing, values in poses:
            for ch, v in values.items():
                tracks.setdefault(ch, []).append((t, v, easing))
        for ch, keys in tracks.items():
            if keys[0][0] > 0:
                keys.insert(0, (0, self.REST[ch], "linear"))
            if ch == "lift" and self.group is None:
                self.group = s.cli("arrange", "group", s.file, "--nodes", ",".join(self.parts),
                                   "--name", self.name)["id"]
            for target in [self.group] if ch == "lift" else self.parts:
                for t, v, easing in keys:
                    s.steps.append(f"keyframe --clip {clip} --target {target} --property {self.PROPERTY[ch]} "
                                   f"--time {num(t)} --value {num(-v if ch == 'lift' else v)} --easing {easing}")
        end = poses[-1][0]
        if self.shadow:
            shadow, opacity, highest, smallest, faintest = self.shadow
            lift = tracks.get("lift", [(0, 0, "linear")])
            wide = tracks.get("sx", [(0, 1, "linear")])

            def ground(t):
                up = min(max(sample(lift, t) / highest, 0), 1)
                size = 1 - (1 - smallest) * up
                return size * sample(wide, t), size, opacity * (1 - (1 - faintest) * up)

            self._derived(clip, shadow, [lift, wide], end, ground, ("scaleX", "scaleY", "opacity"))
        if "turn" in tracks:
            turn = tracks["turn"]
            for rim, (tone, turned) in self.rims.items():
                def lit(t, tone=tone, turned=turned):
                    """Its own tone where it was drawn, the other one when turned half round."""
                    return (mix(turned, tone, (1 + math.cos(math.radians(sample(turn, t)))) / 2),)

                self._derived(clip, rim, [turn], end, lit, ("fill",))
        if autoplay or loop:
            s.steps.append(f"clip update --id {clip}" + " --autoplay" * autoplay + " --loop" * loop)

    def _derived(self, clip, target, tracks, end, fn, props, fps=30):
        """Key `props` of `target` to fn(t), a tuple of numbers or #RRGGBB colours that depends on
        the body's `tracks`: sampled `fps` times a second and at every key of `tracks`, linear in
        between, so it stays in sync whatever the easings. Samples on a straight line are left out."""
        grid = [i / fps for i in range(int(end * fps) + 1)] + [end]
        times = sorted({round(t, 3) for t in [k[0] for keys in tracks for k in keys] + grid})
        values = [fn(t) for t in times]
        flat = [[c / 255 for v in vs for c in rgb(v)] if isinstance(vs[0], str) else list(vs) for vs in values]
        keep = [0]
        for i in range(1, len(times) - 1):
            j = keep[-1]
            share = (times[i] - times[j]) / (times[i + 1] - times[j])
            if any(abs(a + (c - a) * share - b) > 1e-4 for a, b, c in zip(flat[j], flat[i], flat[i + 1])):
                keep.append(i)
        keep.append(len(times) - 1)
        for i in keep:
            for prop, value in zip(props, values[i]):
                value = f"--color {value}" if isinstance(value, str) else f"--value {num(value)}"
                self.scene.steps.append(f"keyframe --clip {clip} --target {target} --property {prop} "
                                        f"--time {num(times[i])} {value} --easing linear")

    def breathe(self, clip, scale, period=2.4):
        """A calm, endless swell and back."""
        self.animate(clip, [(0, "ease-in-out", {"sx": 1, "sy": 1}),
                            (period / 2, "ease-in-out", {"sx": scale, "sy": scale}),
                            (period, "ease-in-out", {"sx": 1, "sy": 1})])

    def swing(self, clip, degrees, period):
        """A calm, endless back-and-forth turn around the pivot."""
        self.animate(clip, [(0, "ease-in-out", {"turn": -degrees}),
                            (period / 2, "ease-in-out", {"turn": degrees}),
                            (period, "ease-in-out", {"turn": -degrees})])

    def spin(self, clip, seconds=0.7):
        """One full turn, for an interaction to play (no autoplay)."""
        self.animate(clip, [(0, "ease-in-out", {"turn": 0}), (seconds, "linear", {"turn": 360})],
                     autoplay=False, loop=False)

    def hop(self, clip, peak, height, duration, up=0.2, down=0.36):
        """One hop per loop: rises for `up` s to `height` px at `peak`, lands `down` s later."""
        start = max(0.0, round(peak - up, 3))
        self.animate(clip, [(start, "ease-out", {"lift": 0}), (peak, "ease-in", {"lift": height}),
                            (round(peak + down, 3), "linear", {"lift": 0}), (duration, "linear", {"lift": 0})])


# ---------------------------------------------------------------- backdrops (512 x 384)

def halle():
    s = Scene("halle", 512, 384, "Eingangshalle")
    s.add("rect", "wall", x=0, y=0, width=512, height=384, gradient=f"linear 90 {INDIGO} {INDIGO_SH}")
    arch = s.add("path", "arch", d="M-110 160 L-110 -20 C -110 -110 -55 -150 0 -150 C 55 -150 110 -110 110 -20 L110 160 Z",
                 tx=256, ty=150, fill=INDIGO_HI)
    s.crescent(arch, -12, -10, INDIGO)
    s.add("path", "door", d="M-80 150 L-80 -10 C -80 -80 -40 -110 0 -110 C 40 -110 80 -80 80 -10 L80 150 Z",
          tx=256, ty=160, gradient=f"linear 90 {INDIGO_DEEP} #12133A")
    s.glow("door_glow", 256, 200, 110, 0.35)
    floor = s.add("path", "floor", d="M0 0 L512 0 L512 74 L0 74 Z", ty=310, fill=INDIGO_SH)
    s.crescent(floor, 0, 8, INDIGO_DEEP)
    # A soft carpet runner; the loci themselves are placed by the app (marker .sply).
    runner = s.add("path", "runner", d="M-60 0 L60 0 L90 74 L-90 74 Z", tx=256, ty=310, fill="#A8384F")
    s.crescent(runner, -8, 0, "#8A2E42")
    s.pulse("door", ["door_glow"], low=0.6, period=3.2)
    s.finish("Eingangshalle: ein hoher Torbogen mit warmem Licht, davor ein Läufer.")


def bibliothek():
    s = Scene("bibliothek", 512, 384, "Bibliothek")
    s.add("rect", "wall", x=0, y=0, width=512, height=384, gradient=f"linear 90 {INDIGO_SH} {INDIGO_DEEP}")
    colors = [LIME, CREAM, INDIGO_HI, AMBER, RED, CREAM, TRIM, INDIGO_HI]
    for row, y in enumerate((70, 170, 270)):
        shelf = s.add("path", f"shelf{row}", d="M0 0 L432 0 L432 14 L0 14 Z", tx=40, ty=y + 70, fill=BROWN)
        s.crescent(shelf, 0, -5, BROWN_SH)
        x = 52
        for b in range(12):
            w = 18 + (b * 7 + row * 5) % 14
            h = 48 + (b * 11 + row * 3) % 20
            s.add("rect", f"book{row}_{b}", x=x, y=y + 70 - h, width=w, height=h, rx=3,
                  fill=colors[(b + row) % len(colors)], opacity=0.92)
            x += w + 6
            if x > 440:
                break
    s.glow("lamp_glow", 420, 40, 70, 0.5)
    s.light("lamp", 420, 40, 12)
    s.pulse("lamp", ["lamp_glow"])
    s.finish("Bibliothek: Regale mit bunten Büchern im Licht einer Lampe.")


def garten():
    s = Scene("garten", 512, 384, "Garten")
    s.add("rect", "sky", x=0, y=0, width=512, height=384, gradient=f"linear 90 {INDIGO_HI} {INDIGO_SH}")
    s.add("circle", "moon", x=420, y=70, radius=30, fill=CREAM)
    s.add("circle", "moon_shade", x=432, y=64, radius=26, fill=INDIGO_HI, opacity=0.9)
    hill = s.add("path", "hill", d="M0 80 C 120 10 300 0 512 60 L512 160 L0 160 Z", ty=224, fill=GREEN)
    s.crescent(hill, -10, -12, GREEN_SH)
    s.crescent(hill, 8, 10, GREEN_HI)
    # Stars only twinkle; the loci are placed by the app (marker .sply).
    stars = []
    for i, (x, y, r) in enumerate([(70, 60, 5), (160, 40, 4), (250, 80, 5), (330, 30, 4), (480, 140, 4)]):
        stars.append(s.add("star", f"star{i}", x=x, y=y, radius=r, tips=4, inner=0.4, fill=CREAM, pivot="center"))
    s.pulse("stars", stars, low=0.35, period=2.8)
    s.finish("Garten am Abend: ein Hügel unter Mond und funkelnden Sternen.")


# ---------------------------------------------------------------- locus pictures (384 x 384)

def teppich():
    s = Scene("teppich", 384, 384, "Route")
    rug = s.add("path", "rug", d=rounded(300, 180, 24), tx=42, ty=110, fill=RED)
    s.crescent(rug, -10, -10, "#A8384F")
    s.add("path", "rug_border", d=rounded(270, 150, 18), tx=57, ty=125, fill="none")
    s.update("rug_border", stroke=AMBER, stroke_width=4, opacity=0.6)
    groups = []
    for i, (x, y) in enumerate([(110, 230), (192, 170), (274, 230)]):
        s.glow(f"g{i}", x, y, 34)
        s.light(f"l{i}", x, y, 20)
        s.add("text", f"n{i}", x=x - 6, y=y + 8, text=str(i + 1), font_size=22, fill=INK)
        groups.append([f"g{i}", f"l{i}"])
    s.sequence("route", groups)
    s.finish("Ein Teppich mit drei nummerierten Orten, die nacheinander aufleuchten: die Route gibt die Reihenfolge.")


def laterne():
    s = Scene("laterne", 384, 384, "Laterne")
    s.glow("halo", 192, 220, 120, 0.7)
    s.add("path", "string", d="M-2 0 L2 0 L2 70 L-2 70 Z", tx=192, ty=40, fill=INDIGO_SH)
    cap = s.add("path", "cap", d="M-46 0 L46 0 L34 28 L-34 28 Z", tx=192, ty=110, fill=INDIGO)
    s.crescent(cap, -6, -6, INDIGO_SH)
    s.add("rect", "glass", x=150, y=138, width=84, height=120, rx=26, gradient=f"radial {GLOW_CORE}@0 #F3FF9A@0.5 #BEEB3C@1")
    s.add("star", "star", x=192, y=198, radius=26, tips=4, inner=0.35, fill="#FFFFFF", pivot="center")
    base = s.add("path", "base", d="M-34 0 L34 0 L46 22 L-46 22 Z", tx=192, ty=258, fill=INDIGO_SH)
    s.pulse("glow", ["halo"])
    s.steps += ["keyframe --clip spin_anim --target star --property rotation --time 3 --value 90 --autoplay --loop"]
    s.finish("Eine leuchtende Laterne mit einem funkelnden Stern: ein lebhaftes Bild für jeden Ort.")
    return base


def regal():
    s = Scene("regal", 384, 384, "Seltsames Buch")
    book = s.add("path", "book", d=rounded(170, 220, 14), tx=107, ty=90, fill=INDIGO_HI)
    s.crescent(book, -12, -10, INDIGO)
    s.add("path", "pages", d="M0 0 L14 0 L14 200 L0 200 Z", tx=263, ty=100, fill=CREAM)
    shut = []
    for side, x in (("l", 160), ("r", 224)):
        s.add("circle", f"eye_{side}", x=x, y=180, radius=26, fill="#FBF6EE", pivot="center")
        s.add("circle", f"pupil_{side}", x=x + 6, y=184, radius=11, fill="#1E0B02")
        s.add("circle", f"shine_{side}", x=x + 1, y=178, radius=4, fill="white")
        # The eye closes around its centre, pupil and shine with it; pupil and shine look the same way.
        eye = s.body(f"Auge {side}", [f"eye_{side}", f"pupil_{side}", f"shine_{side}"], (x, 180))
        shut += eye.states("scaleY", 0.1)
        # `follow` still offsets pupil and shine while the lid is shut: they fade out, so they
        # never sit on the cover above or below the closed eye.
        shut += [f"--state {n}_{side}.opacity=0" for n in ("pupil", "shine")]
        s.steps += [f"follow --target {n}_{side} --max 6 --strength 0.15 --smoothing 0.1 --origin {x},180"
                    for n in ("pupil", "shine")]
    s.add("path", "mouth", d="M-26 0 Q 0 26 26 0 L22 6 Q 0 30 -22 6 Z", tx=192, ty=240, fill="#1E0B02")
    s.add("star", "spark", x=300, y=90, radius=18, tips=4, inner=0.35, fill=LIME, pivot="center")
    s.steps += [f"on --machine blink --from open --to shut --after 2.6 {' '.join(shut)} --blend 0.06",
                "on --machine blink --from shut --to open --after 0.14 --blend 0.06"]
    s.pulse("spark", ["spark"], low=0.2, period=1.6)
    s.finish("Ein Buch mit großen Augen und breitem Grinsen: je seltsamer das Bild, desto besser bleibt es hängen.")


def leseplatz():
    s = Scene("leseplatz", 384, 384, "Abrufen")
    page_l = s.add("path", "page_l", d="M0 0 C 40 -10 80 -6 110 8 L110 100 C 80 86 40 82 0 92 Z", tx=82, ty=230, fill=CREAM)
    s.crescent(page_l, 8, 8, CREAM_SH)
    s.add("path", "page_r", d="M0 8 C 30 -6 70 -10 110 0 L110 92 C 70 82 30 86 0 100 Z", tx=192, ty=230, fill=CREAM_SH)
    bubble = s.add("path", "bubble", d=ellipse(120, 70), tx=192, ty=110, fill="#FBF6EE")
    s.crescent(bubble, -10, -8, CREAM_SH)
    s.add("circle", "tail1", x=150, y=200, radius=14, fill="#FBF6EE")
    s.add("circle", "tail2", x=130, y=226, radius=8, fill="#FBF6EE")
    dots = []
    for i, x in enumerate((152, 192, 232)):
        s.light(f"dot{i}", x, 110, 13)
        dots.append([f"dot{i}"])
    s.sequence("think", dots, step=0.4, hold=2.0, cycle=2.6, low=0.15)
    s.finish("Ein aufgeschlagenes Buch mit einer Gedankenblase: beim Üben gehst du die Route ab und rufst jeden Ort ab.")


def uhr():
    s = Scene("uhr", 384, 384, "Wiederholen")
    for i, r in enumerate((150, 125, 100)):
        s.add("path", f"arc{i}", d=f"M{-r} 0 A {r} {r} 0 0 1 {r} 0", tx=192, ty=200, fill="none")
        s.update(f"arc{i}", stroke=LIME, stroke_width=6 - i, opacity=0.35 + 0.2 * i)
    face = s.add("circle", "face", x=192, y=200, radius=82, fill=CREAM)
    s.crescent(face, -10, -10, CREAM_SH)
    for i in range(12):
        import math
        a = math.radians(i * 30)
        s.add("circle", f"tick{i}", x=round(192 + 66 * math.sin(a), 1), y=round(200 - 66 * math.cos(a), 1), radius=4 if i % 3 else 6, fill=INK)
    s.add("path", "hand", d="M-4 8 L-3 -58 L0 -64 L3 -58 L4 8 Z", tx=192, ty=200, fill=RED)
    s.add("circle", "hub", x=192, y=200, radius=9, fill=INK)
    s.steps += ["keyframe --clip tick_anim --target hand --property rotation --time 6 --value 360 --autoplay --loop"]
    s.sequence("spacing", [["arc2"], ["arc1"], ["arc0"]], step=0.7, hold=3.0, cycle=3.6, low=0.1)
    s.finish("Eine Uhr mit wachsenden Bögen: wiederhole kurz bevor du vergisst, mit immer größeren Abständen.")


def brunnen():
    s = Scene("brunnen", 384, 384, "Ehrlich bewerten")
    labels = [("1", RED, "Nochmal"), ("2", AMBER, "Schwer"), ("3", TRIM, "Gut"), ("4", INDIGO_HI, "Leicht")]
    groups = []
    for i, (n, color, word) in enumerate(labels):
        y = 60 + i * 72
        key = s.add("path", f"key{i}", d=rounded(220, 56, 16), tx=82, ty=y, fill=color)
        s.crescent(key, -6, -6, INK, opacity=0.25)
        s.add("text", f"num{i}", x=100, y=y + 38, text=n, font_size=28, fill=INK)
        s.add("text", f"word{i}", x=140, y=y + 37, text=word, font_size=24, fill=INK)
        groups.append([f"key{i}"])
    s.sequence("keys", groups, step=0.45, hold=2.4, cycle=3.0, low=0.55)
    s.finish("Vier Tasten von 1 Nochmal bis 4 Leicht: bewerte ehrlich, wie gut du dich erinnert hast.")


def baum():
    s = Scene("baum", 384, 384, "Eigener Palast")
    trunk = s.add("path", "trunk", d="M-22 0 L-16 -120 L16 -120 L22 0 Z", tx=192, ty=340, fill=BROWN)
    s.crescent(trunk, -8, 0, BROWN_SH)
    parts = [s.add("circle", f"crown{i}", x=x, y=y, radius=r) for i, (x, y, r) in
             enumerate([(192, 130, 90), (120, 170, 60), (264, 170, 60), (150, 100, 56), (236, 100, 56)])]
    crown = s.cli("arrange", "boolean", s.file, "--op", "union", "--nodes", ",".join(parts))["id"]
    s.update(crown, fill=GREEN)
    s.crescent(crown, -14, -12, GREEN_SH)
    s.crescent(crown, 10, 10, GREEN_HI)
    groups = []
    for i, (x, y) in enumerate([(140, 150), (200, 104), (252, 160), (168, 190), (230, 186)]):
        s.glow(f"fg{i}", x, y, 22)
        s.light(f"fruit{i}", x, y, 10)
        groups.append([f"fg{i}", f"fruit{i}"])
    s.sequence("grow", groups, step=0.45, hold=3.0, cycle=3.6, low=0.0)
    s.finish("Ein Baum, an dem leuchtende Früchte wachsen: lege deine eigenen Paläste an.")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for build in (halle, bibliothek, garten, teppich, laterne, regal, leseplatz, uhr, brunnen, baum):
        build()
