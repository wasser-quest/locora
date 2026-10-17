#!/usr/bin/env python3
"""Build the three small Journey progress symbols in Locora's cel-shaded style.

Usage: SPLINELY_CLI=splinely-cli python3 assets/ui/src/gamification.py [OUTPUT_DIR]
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "tutorial", "src"))
import tutorial as t
from tutorial import Scene

t.OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))


def herz():
    s = Scene("herz", 96, 96, "Herz")
    shape = s.add("path", "heart", d="M0 25 C-15 11 -35 -3 -35 -20 C-35 -34 -25 -42 -13 -42 C-6 -42 0 -38 0 -31 C0 -38 6 -42 13 -42 C25 -42 35 -34 35 -20 C35 -3 15 11 0 25 Z", tx=48, ty=57, fill=t.RED)
    s.crescent(shape, -5, -5, "#A8384F")
    s.crescent(shape, 4, 4, "#EA7386")
    s.glow("memory", 48, 45, 15, 0.45)
    s.light("locus", 48, 45, 5)
    s.pulse("locus", ["memory"], low=0.5, period=3.0)
    s.finish("Ein rotes Herz mit einem sanft leuchtenden Locus.")


def combo():
    s = Scene("combo", 96, 96, "Combo")
    disc = s.add("circle", "disc", x=48, y=48, radius=34, fill=t.INDIGO, pivot="center")
    s.crescent(disc, -6, -6, t.INDIGO_SH)
    s.crescent(disc, 4, 4, t.INDIGO_HI)
    dots = []
    for i, (x, y) in enumerate(((32, 55), (48, 43), (65, 30))):
        s.glow(f"glow{i}", x, y, 12, 0.6)
        s.light(f"dot{i}", x, y, 5)
        dots.append([f"glow{i}", f"dot{i}"])
    s.sequence("combo", dots, step=0.5, hold=2.4, cycle=3.0, low=0.25)
    s.finish("Drei Loci steigen nacheinander auf: eine Serie richtiger Antworten.")


def pokal():
    s = Scene("pokal", 96, 96, "Erfolg")
    s.glow("halo", 48, 38, 37, 0.5)
    cup = s.add("path", "cup", d="M-23 -27 L23 -27 L19 -1 C17 13 8 21 0 21 C-8 21 -17 13 -19 -1 Z", tx=48, ty=40, fill=t.AMBER)
    s.crescent(cup, -5, -5, "#B98A3A")
    s.crescent(cup, 4, 4, "#F6DC9C")
    for side in (-1, 1):
        handle = s.add("circle", f"handle{side}", x=48 + side * 24, y=27, radius=11, fill=t.AMBER)
        s.crescent(handle, -3, -3, "#B98A3A")
    s.add("rect", "stem", x=44, y=62, width=8, height=11, rx=2, fill=t.INDIGO)
    base = s.add("rect", "base", x=31, y=72, width=34, height=7, rx=3, fill=t.INDIGO)
    s.crescent(base, -3, -2, t.INDIGO_SH)
    s.light("locus", 48, 39, 6)
    s.pulse("win", ["halo"], low=0.55, period=3.0)
    s.finish("Ein goldener Pokal mit einem leuchtenden Locus für erreichte Ziele.")


if __name__ == "__main__":
    os.makedirs(t.OUT, exist_ok=True)
    for build in (herz, combo, pokal):
        build()
