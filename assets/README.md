# Assets

All graphics are made with [Splinely](https://splinely.com) via `splinely-cli` and shipped as `.sply` files. The app plays them with the Splinely runtime. The `.svg`, `.gif` and `.png` files are exports for places without the runtime (README, stores, favicons). Regenerate them from the `.sply` source; do not edit them by hand.

| File | What |
|---|---|
| `mascot/simo.sply` | **Simo**, the Locora mascot, named after Simonides of Ceos, who is credited with inventing the method of loci. Full body, no outlines, cel-shaded: light brown skin, a compact afro with a clean hairline, a lantern full of firefly light and three loci on his jacket that light up 1 → 2 → 3. He breathes, blinks and follows the pointer with his eyes; the lantern sways like a pendulum; clicking his face sends the firefly on a loop. Built by `mascot/src/simo.py`. |
| `mascot/gluehwuermchen.sply` | Simo's firefly on its own, in the same lineless style as next to Simo: hovers, flaps, glows, blinks, follows the pointer with its eyes and hops on click, while a route of three numbered loci lights up 1 → 2 → 3. Built by `mascot/src/gluehwuermchen.py`. |
| `tutorial/*.sply` | Graphics of the tutorial palace „Locora lernen“: three room backdrops (Eingangshalle, Bibliothek, Garten; 512 × 384) and seven locus pictures (Teppich, Laterne, Regal, Leseplatz, Uhr, Brunnen, Baum; 384 × 384), all animated. Built by `tutorial/src/tutorial.py`. |
| `tutorial/locora-lernen.locora.toml` | The tutorial palace itself; every place shows a `.sply` (Simo and the firefly come from `mascot/`). Built into the CLI: `locora tutorial`. |
| `marker/locus.sply` | Default locus marker: a glowing point with the states `neu`, `faellig`, `gewusst` (state machine `status`). The app places it at every locus; backdrops never contain loci. Built by `marker/src/marker.py`. |
| `brand/icon.sply` | App icon: a palace arch with a route of glowing loci and the firefly's light. |

Style guide: [`STIL.md`](STIL.md) (German).

```bash
SPLINELY_CLI=splinely-cli python3 mascot/src/simo.py mascot/simo.sply   # rebuild Simo from source
splinely-cli export mascot/simo.sply --output mascot/simo.svg   # interactive SVG
splinely-cli render mascot/simo.sply --output mascot/simo.gif --duration 3.6   # animated preview
SPLINELY_CLI=splinely-cli python3 mascot/src/gluehwuermchen.py mascot/gluehwuermchen.sply   # rebuild the firefly
SPLINELY_CLI=splinely-cli python3 tutorial/src/tutorial.py                                     # rebuild the tutorial graphics
splinely-cli render brand/icon.sply --time 0 --output brand/icon.png
```
