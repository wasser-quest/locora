# Assets

All graphics are made with [Splinely](https://splinely.com) via `splinely-cli` and shipped as `.sply` files. The app plays them with the Splinely runtime. The `.svg`, `.gif` and `.png` files are exports for places without the runtime (README, stores, favicons). Regenerate them from the `.sply` source; do not edit them by hand.

| File | What |
|---|---|
| `mascot/mnemo.sply` | **Mnemo**, the Locora mascot, named after *mnemo*nics: a character with brown skin, a big afro, a loci hairpin whose three lights glow one after another (1 → 2 → 3) and a firefly companion. Mnemo breathes, blinks, follows the pointer with their eyes; clicking the face sends the firefly on a loop. Built from `mascot/src/build.py` via `mascot/src/make.sh`. |
| `mascot/simo.sply` | **Simo**, named after Simonides of Ceos, who is credited with inventing the method of loci: a full-body character with light brown skin and an afro, a lantern full of firefly light and three loci on his jacket that light up 1 → 2 → 3. Breathes, blinks, follows the pointer with his eyes; the lantern sways like a pendulum. Built from `mascot/src/build_simo.py` via `mascot/src/make_simo.sh`. |
| `mascot/gluehwuermchen.sply` | Mnemo's firefly on its own: lights up a numbered route of loci; hovers, flaps, glows, blinks and hops on click. |
| `brand/icon.sply` | App icon: a palace arch with a route of glowing loci and the firefly's light. |

Style guide: [`STIL.md`](STIL.md) (German).

```bash
SPLINELY_CLI=splinely-cli mascot/src/make.sh "$PWD/mascot/mnemo.sply"   # rebuild Mnemo from source
splinely-cli export mascot/mnemo.sply --output mascot/mnemo.svg   # interactive SVG
splinely-cli render mascot/mnemo.sply --output mascot/mnemo.gif   # animated preview
splinely-cli render brand/icon.sply --time 0 --output brand/icon.png
```
