"""Shared building blocks for Locora's characters (Simo's friends).

Same rules as Simo (assets/mascot/src/simo.py, assets/STIL.md): no outlines, every
part is a flat base colour plus one hard shadow (bottom right) and at most one
light edge (top left), made with boolean operations so shadows follow the
silhouette. Light comes from the top left. Canvas 512 x 768, feet at y ~ 706.

A character script creates a `Figure`, draws its body with the helpers below
(legs, torso, arms, face, eyes …), adds its own hair, headwear and prop, then
calls `rig` and `animate` with its signature move.
"""
import json, os, re, subprocess

CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")

EYE_WHITE = "#FBF6EE"
LIME, TRIM = "#DDF76A", "#C9E85A"
GLOW = "radial rgba(230,255,120,0.8)@0 rgba(230,255,120,0)@1"
GLOW_CORE = f"radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1"


def ellipse(rx, ry):
    k = 0.5523
    return (f"M{-rx:g} 0 C {-rx:g} {-ry*k:.2f} {-rx*k:.2f} {-ry:g} 0 {-ry:g} C {rx*k:.2f} {-ry:g} {rx:g} {-ry*k:.2f} {rx:g} 0 "
            f"C {rx:g} {ry*k:.2f} {rx*k:.2f} {ry:g} 0 {ry:g} C {-rx*k:.2f} {ry:g} {-rx:g} {ry*k:.2f} {-rx:g} 0 Z")


def tf(d, sx=1, sy=1, dx=0, dy=0):
    """Scales and moves an absolute path (M/L/Q/C/Z with number pairs)."""
    res, ix = [], True
    for tok in re.findall(r"[A-Za-z]|-?\d*\.?\d+", d):
        if tok.isalpha():
            res.append(tok); ix = True; continue
        v = float(tok) * (sx if ix else sy) + (dx if ix else dy)
        res.append(f"{v:.2f}".rstrip("0").rstrip(".")); ix = not ix
    return " ".join(res)


def mx(d):
    return tf(d, -1, 1)


def same(d):
    return d


def capsule(w, l):
    r = w / 2
    return f"M{-r:g} 0 C {-r:g} {-r*1.1:.1f} {r:g} {-r*1.1:.1f} {r:g} 0 L{r:g} {l:g} C {r:g} {l+r*1.1:.1f} {-r:g} {l+r*1.1:.1f} {-r:g} {l:g} Z"


SIMO_FACE = ("M-92 -20 C -92 -70 -52 -92 0 -92 C 52 -92 92 -70 92 -20 C 92 30 86 60 68 84 C 50 104 28 114 0 116 "
             "C -28 114 -50 104 -68 84 C -86 60 -92 30 -92 -20 Z")
# A softer, rounder face (child, older woman).
ROUND_FACE = ("M-92 -14 C -92 -68 -52 -92 0 -92 C 52 -92 92 -68 92 -14 C 92 38 80 70 60 90 C 42 106 22 112 0 112 "
              "C -22 112 -42 106 -60 90 C -80 70 -92 38 -92 -14 Z")
JACKET = "M-72 0 C -104 4 -126 20 -128 52 L-110 186 C -66 196 66 196 110 186 L128 52 C 126 20 104 4 72 0 Z"
SHOE = "M-40 4 C -40 -18 20 -22 36 -6 C 44 4 40 18 24 18 L-30 18 C -40 18 -42 12 -40 4 Z"
SOLE = "M-38 12 L36 12 C 40 12 38 18 24 18 L-30 18 C -40 18 -42 14 -38 12 Z"


class Figure:
    """One .sply character. `hy` is the head centre's y (Simo: 256)."""

    def __init__(self, path, name, palette, hy=256):
        self.F = os.path.abspath(path)
        self.P = palette
        self.hy = hy
        self.arm_parts = {"l": [], "r": []}
        self.eye_parts = {}
        self.front = []  # nodes moved to the front at the end (hair, headwear)
        if os.path.exists(self.F):
            os.remove(self.F)
        self.cli("new", "--output", self.F, "--width", "512", "--height", "768", "--name", name)

    # ------------------------------------------------------------ CLI wrappers
    def cli(self, *args):
        out = subprocess.run([CLI, *args], capture_output=True, text=True)
        if out.returncode != 0:
            raise SystemExit(f"splinely-cli {' '.join(args)}\n{out.stdout}{out.stderr}")
        return json.loads(out.stdout) if out.stdout.strip() else {}

    def add(self, kind, id, **o):
        args = ["node", "add", self.F, "--kind", kind, "--id", id]
        for k, v in o.items():
            args += [f"--{k.replace('_', '-')}", str(v)]
        self.cli(*args)
        return id

    def update(self, id, **o):
        args = ["node", "update", self.F, id]
        for k, v in o.items():
            args += [f"--{k.replace('_', '-')}", str(v)]
        self.cli(*args)

    def dup(self, id, dx=0, dy=0):
        return self.cli("node", "duplicate", self.F, "--nodes", id, "--dx", str(dx), "--dy", str(dy))["created"][0]

    def boolean(self, op, ids):
        return self.cli("arrange", "boolean", self.F, "--op", op, "--nodes", ",".join(ids))["id"]

    def crescent(self, base, dx, dy, fill, opacity=None):
        """The part of `base` not covered by a copy shifted by (dx, dy): a rim that follows the silhouette."""
        a, b = self.dup(base), self.dup(base, dx, dy)
        self.cli("arrange", "order", self.F, "--nodes", b, "--to", "front")
        res = self.boolean("difference", [a, b])
        self.update(res, fill=fill, **({"opacity": opacity} if opacity is not None else {}))
        return res

    def cast(self, target, caster, dx, dy, fill, opacity=None):
        """Shadow of `caster` shifted by (dx, dy), kept only where it falls on `target`."""
        a, b = self.dup(target), self.dup(caster, dx, dy)
        res = self.boolean("intersection", [a, b])
        self.update(res, fill=fill, **({"opacity": opacity} if opacity is not None else {}))
        return res

    def shaded(self, id, d, fill, sh, hi=None, sh_off=(-14, -12), hi_off=(10, 10), **o):
        """A path with its hard shadow and optional light edge; returns all ids."""
        base = self.add("path", id, d=d, fill=fill, **o)
        parts = [base, self.crescent(base, *sh_off, sh)]
        if hi:
            parts.append(self.crescent(base, *hi_off, hi))
        return parts

    def to_front(self, ids):
        self.cli("arrange", "order", self.F, "--nodes", ",".join(ids), "--to", "front")

    def y(self, v):
        """A y from Simo's head layout, moved to this figure's head."""
        return v - 256 + self.hy

    # ------------------------------------------------------------ body
    def legs(self, top, fill, sh, shoe, shoe_sh, stripe, width=54, spread=28, sock=None):
        length = 672 - top
        for side, x, rot in (("l", 256 - spread, 2), ("r", 256 + spread, -2)):
            leg = self.add("path", f"leg_{side}", d=capsule(width, length), tx=x, ty=top, rotation=rot, fill=fill)
            self.crescent(leg, -12, -6, sh)
            if sock:
                self.add("path", f"sock_{side}", d=f"M{-width/2+1:g} 0 L{width/2-1:g} 0 L{width/2-1:g} 18 L{-width/2+1:g} 18 Z",
                         tx=x, ty=666, fill=sock)
        for side, x, f in (("l", 256 - spread - 8, mx), ("r", 256 + spread + 8, same)):
            s = self.add("path", f"shoe_{side}", d=f(SHOE), tx=x, ty=688, fill=shoe)
            self.crescent(s, -8, -8, shoe_sh)
            self.add("path", f"sole_{side}", d=f(SOLE), tx=x, ty=688, fill=shoe_sh)
            if stripe:
                self.add("path", f"stripe_{side}", d=f("M-24 2 L18 2 L16 7 L-24 7 Z"), tx=x, ty=688, fill=stripe)

    def neck(self, top=None, width=30):
        top = self.y(342) if top is None else top
        return self.add("path", "neck", d=f"M{-width} 0 L{-width} 54 L{width} 54 L{width} 0 Z", tx=256, ty=top,
                        fill=self.P["skin_sh"])

    def torso(self, d, top, fill, sh, hi):
        jacket = self.add("path", "body", d=d, tx=256, ty=top, fill=fill)
        self.crescent(jacket, -18, -12, sh)
        self.crescent(jacket, 10, 10, hi)
        return jacket

    def arm(self, side, length, sleeve, sleeve_sh, cuff=None, width=46, hand_r=18):
        """Bone-local arm (origin = shoulder). Returns the hand's distance from the shoulder."""
        sl = self.add("path", f"sleeve_{side}", d=capsule(width, length), fill=sleeve)
        self.arm_parts[side] += [sl, self.crescent(sl, -12, -4, sleeve_sh)]
        if cuff:
            w = width / 2 + 1
            self.arm_parts[side].append(self.add("path", f"cuff_{side}", d=f"M{-w:g} 0 L{w:g} 0 L{w:g} 9 L{-w:g} 9 Z",
                                                 ty=length - 12, fill=cuff))
        return length + 14

    def hand(self, side, at):
        h = self.add("circle", f"hand_{side}", x=0, y=at, radius=18, fill=self.P["skin"])
        self.arm_parts[side] += [h, self.crescent(h, -6, -6, self.P["skin_sh"])]

    # ------------------------------------------------------------ head
    def ears(self, xs=(164, 348), y=278):
        for side, x in zip("lr", xs):
            self.add("path", f"ear_{side}", d=ellipse(14, 20), tx=x, ty=self.y(y),
                     fill=self.P["skin"] if side == "l" else self.P["skin_sh"])
            self.add("path", f"ear_in_{side}", d=ellipse(6, 11), tx=x + (2 if side == "l" else -2), ty=self.y(y + 1),
                     fill=self.P["skin_deep"])

    def face(self, d=SIMO_FACE, neck=None):
        P = self.P
        f = self.add("path", "face", d=d, tx=256, ty=self.hy, fill=P["skin"])
        if neck:
            self.cast(neck, f, 0, 14, P["skin_deep"])
        self.crescent(f, -16, 0, P["skin_sh"])
        self.crescent(f, 8, 10, P["skin_hi"])
        return f

    def eyes(self, iris, pupil, y=284, dx=42, brow_y=244, brow=None, brow_fill=None, lid=None):
        """Almond eyes with a strong upper lid, one highlight top left and a dot; plus closed lids and brows."""
        P = self.P
        sclera = "M-25 -6 C -17 -19 15 -21 25 -9 C 27 4 18 15 0 17 C -18 15 -27 4 -25 -6 Z"
        brow = brow or "M-24 4 L-20 -4 L22 -6 L24 2 Z"
        lid = lid or "M-28 -6 C -18 -24 16 -26 28 -11 L30 -8 C 18 -18 -16 -16 -27 0 Z"
        for side, x, f in (("l", 256 - dx, mx), ("r", 256 + dx, same)):
            yy = self.y(y)
            parts = [
                self.add("path", f"eye_{side}", d=f(sclera), tx=x, ty=yy, fill=EYE_WHITE),
                self.add("path", f"iris_{side}", d=ellipse(14, 16), tx=x + 1, ty=yy, gradient=f"linear 90 {iris}"),
                self.add("path", f"pupil_{side}", d=ellipse(6.5, 8), tx=x + 1, ty=yy - 1, fill=pupil),
                self.add("path", f"hl1_{side}", d=ellipse(4.5, 5.5), tx=x - 5, ty=yy - 6, fill="white"),
                self.add("circle", f"hl2_{side}", x=x + 7, y=yy + 7, radius=2.2, fill="white"),
                self.add("path", f"lid_{side}", d=f(lid), tx=x, ty=yy, fill=P["feature"]),
                self.add("path", f"lower_{side}", d=f("M-14 16 Q 0 20 12 15 L12 17 Q 0 23 -14 18 Z"), tx=x, ty=yy,
                         fill=P["skin_deep"]),
            ]
            self.eye_parts[side] = parts
            self.add("path", f"closed_{side}", d=f("M-26 0 Q 0 12 28 -2 L28 2 Q 0 17 -26 4 Z"), tx=x, ty=yy,
                     fill=P["feature"], opacity=0)
            self.add("path", f"brow_{side}", d=f(brow), tx=x, ty=self.y(brow_y), fill=brow_fill or P["hair_sh"])

    def nose_mouth(self, mouth=None, nose_y=310, mouth_y=336, chin=True):
        P = self.P
        self.add("path", "nose", d="M3 -8 L-5 6 L3 8 L4 6 L-1 5 L5 -7 Z", tx=257, ty=self.y(nose_y), fill=P["skin_deep"])
        self.add("path", "mouth", d=mouth or "M-18 -2 Q 0 12 18 -4 L19 -1 Q 0 17 -18 1 Z", tx=256, ty=self.y(mouth_y),
                 fill=P["feature"])
        self.add("path", "mouth_corner", d="M18 -4 L22 -8 L23 -6 L19 -1 Z", tx=256, ty=self.y(mouth_y), fill=P["feature"])
        if chin:
            self.add("path", "chin_shadow", d="M-26 0 Q 0 10 26 0 Q 0 6 -26 0 Z", tx=256, ty=self.y(356),
                     fill=P["skin_sh"], opacity=0.8)

    def glow(self, id, x, y, r=5.5, halo=13):
        """A glowing locus (the brand's memory light)."""
        return [self.add("circle", f"{id}_glow", x=x, y=y, radius=halo, gradient=GLOW, pivot="center"),
                self.add("circle", id, x=x, y=y, radius=r, gradient=GLOW_CORE, pivot="center")]

    # ------------------------------------------------------------ rig & motion
    def rig(self, shoulders, extra=()):
        """Torso bone (breathing), one bone per arm at `shoulders` {side: (x, y, rotation)}, and
        `extra` bones as (id, parent, x, y, rotation, nodes[, (dx, dy)]); nodes drawn in scene
        coordinates are first moved by (dx, dy) so they sit around the bone. Everything else hangs
        on the torso."""
        F = self.F
        self.cli("bone", "add", F, "--id", "torso", "--x", "0", "--y", "0", "--length", "1")
        for s, (x, y, rot) in shoulders.items():
            self.cli("bone", "add", F, "--id", f"arm_{s}", "--parent", "torso", "--x", str(x), "--y", str(y),
                     "--rotation", str(rot), "--length", "1")
        bound = set()
        for bid, parent, x, y, rot, nodes, *shift in extra:
            if shift:
                self.cli("arrange", "move", F, "--nodes", ",".join(nodes), "--dx", str(shift[0][0]),
                         "--dy", str(shift[0][1]))
            self.cli("bone", "add", F, "--id", bid, "--parent", parent, "--x", str(x), "--y", str(y),
                     "--rotation", str(rot), "--length", "1")
            for n in nodes:
                self.cli("bone", "bind", F, "--node", n, "--bone", bid); bound.add(n)
        for s in "lr":
            for n in self.arm_parts[s]:
                if n not in bound:
                    self.cli("bone", "bind", F, "--node", n, "--bone", f"arm_{s}"); bound.add(n)
        for n in [n["id"] for n in self.cli("node", "list", F)["nodes"]]:
            if n not in bound:
                self.cli("bone", "bind", F, "--node", n, "--bone", "torso")

    def animate(self, steps, title, description, label):
        """Breathing, blinking and eyes that follow the pointer, plus the figure's own `steps`."""
        base = []
        shut = [f"--state {n}.opacity=0" for s in "lr" for n in self.eye_parts[s]]
        shut += ["--state closed_l.opacity=1", "--state closed_r.opacity=1"]
        base.append(f"on --machine blink --from open --to shut --after 3.8 {' '.join(shut)} --blend 0.06")
        base.append("on --machine blink --from shut --to open --after 0.13 --blend 0.06")
        for s in "lr":
            for n in (f"iris_{s}", f"pupil_{s}", f"hl1_{s}", f"hl2_{s}"):
                base.append(f"follow --target {n} --max 3 --strength 0.12 --smoothing 0.12")
        base += [
            "keyframe --clip breathe --target torso --property y --time 1.8 --by -3 --easing ease-in-out --autoplay --loop",
            "keyframe --clip breathe --target torso --property y --time 3.6 --value 0 --easing ease-in-out",
        ]
        base += steps
        base += [f'a11y --title "{title}" --description "{description}"', f'a11y --node face --label "{label}"']
        tmp = self.F + ".steps"
        with open(tmp, "w") as fh:
            fh.write("\n".join(base) + "\n")
        try:
            self.cli("batch", self.F, "--steps", tmp)
        finally:
            os.remove(tmp)
        print(f"built {self.F}")
