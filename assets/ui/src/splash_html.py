#!/usr/bin/env python3
"""Writes the start splash into apps/app/ui/index.html.

Usage: python3 assets/ui/src/splash_html.py

The four Splinely animations (apps/app/ui/splash/*.svg, exported by
assets/ui/src/splash.py) are inlined as <template>s, so the page has them
before the wasm starts: no download, nothing to wait for. The embedded runtime
keeps its code; only the CDATA wrapper is removed, because script contents in
HTML are plain text. The page script clones one of them at random.
"""
import glob, os, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
UI = os.path.join(ROOT, "apps", "app", "ui")
SPLASH = os.path.join(UI, "splash")


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
    page = f"""<!doctype html>
<html lang="de">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Locora</title>
    <link data-trunk rel="css" href="style.css" />
    <!-- Rust enables these wasm32 features by default; wasm-opt must accept them. -->
    <link data-trunk rel="rust" data-wasm-opt="z"
      data-wasm-opt-params="--enable-bulk-memory --enable-bulk-memory-opt --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types --enable-multivalue --enable-mutable-globals" />
    <!-- Painted before the wasm loads: a random animation and a random tip in the saved language.
         The animations are inlined (assets/ui/src/splash_html.py), so nothing is fetched.
         The app asks for the hide (window.hideSplash) once it is mounted. -->
    <style>
      html, body {{ margin: 0; background: #f3eee3; }}
      #splash {{ position: fixed; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1.25rem; padding: 1rem; color: #22234f; font: 500 1.05rem system-ui, sans-serif; text-align: center; }}
      #splash-art {{ width: min(70vw, 300px); height: min(70vw, 300px); }}
      #splash-art svg {{ width: 100%; height: 100%; }}
      #splash-tip {{ max-width: 22rem; margin: 0; line-height: 1.4; }}
      @media (prefers-color-scheme: dark) {{
        html, body {{ background: #16173a; }}
        #splash {{ color: #ece8dc; }}
      }}
    </style>
  </head>
  <body>
    <div id="splash">
      <div id="splash-art"></div>
      <p id="splash-tip"></p>
    </div>
{templates}
    <script>
      (function () {{
        var scenes = {names!r};
        var tips = {{
          en: [
            "Put what you want to remember at places you know well.",
            "The route is the order: walk it in your mind to recall it.",
            "Make the picture strange: the stranger, the better it sticks.",
            "Review just before you forget: that is when it sticks best.",
            "Next goes to the next place – quick next moves on after your own grade."
          ],
          de: [
            "Lege, was du behalten willst, an Orte, die du gut kennst.",
            "Die Route ist die Reihenfolge: geh sie im Kopf ab, um dich zu erinnern.",
            "Mach das Bild seltsam: je seltsamer, desto besser bleibt es.",
            "Wiederhole kurz bevor du vergisst – dann bleibt es am besten.",
            "„Weiter“ führt zum nächsten Ort – mit Schnell weiter geht es nach deiner Bewertung."
          ],
          fr: [
            "Place ce que tu veux retenir dans des lieux que tu connais bien.",
            "Le parcours est l’ordre : parcours-le dans ta tête pour te souvenir.",
            "Rends l’image étrange : plus elle l’est, mieux elle reste.",
            "Révise juste avant d’oublier : c’est là que ça reste le mieux.",
            "« Suivant » passe au lieu suivant – après ta propre évaluation."
          ],
          ja: [
            "覚えたいものは、よく知っている場所に置きましょう。",
            "ルートは順番そのもの。頭の中でたどると思い出せます。",
            "イメージは奇妙に: 奇妙なほど記憶に残ります。",
            "忘れる直前に復習すると、いちばん定着します。",
            "「次へ」で次の場所へ。自分で評価した後に進みます。"
          ],
          zh: [
            "把想记住的内容放在你熟悉的位置上。",
            "路线就是顺序：在脑中走一遍就能回想起来。",
            "让画面奇怪一点：越奇怪，越容易记住。",
            "在快要忘记时复习，记得最牢。",
            "“下一个”会进入下一个位置——你评价之后才继续。"
          ]
        }};
        var pick = function (items) {{ return items[Math.floor(Math.random() * items.length)]; }};
        var lang = "en";
        try {{ lang = localStorage.getItem("locora-language-v1") || "en"; }} catch (_) {{}}
        var started = Date.now();
        var art = document.getElementById("splash-art");
        var template = document.getElementById(pick(scenes));
        // The animation is already in the page: it is inserted at once.
        if (template) art.appendChild(document.importNode(template.content, true));
        document.getElementById("splash-tip").textContent = pick(tips[lang] || tips.en);
        var hideRequested = false, removed = false;
        // The splash stays for a short time, so the animation can be seen; the app asks for the hide once mounted.
        var minShown = 1800, maxWait = 3000;
        var finish = function () {{
          if (removed) return;
          removed = true;
          var splash = document.getElementById("splash");
          if (splash) splash.remove();
        }};
        var check = function () {{
          if (!hideRequested || removed) return;
          var waited = Date.now() - started;
          if (waited >= minShown || waited >= maxWait) finish();
          else setTimeout(check, 100);
        }};
        window.hideSplash = function () {{ hideRequested = true; check(); }};
      }})();
    </script>
  </body>
</html>
"""
    open(os.path.join(UI, "index.html"), "w", encoding="utf-8").write(page)
    print("wrote index.html with", len(names), "splash animations")


if __name__ == "__main__":
    main()
