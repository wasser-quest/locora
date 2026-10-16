# Assets

All graphics are made with [Splinely](https://splinely.com) via `splinely-cli` and shipped as `.sply` files. The app plays them with the Splinely runtime. The `.svg`, `.gif` and `.png` files are exports for places without the runtime (README, stores, favicons). Regenerate them from the `.sply` source; do not edit them by hand.

| File | What |
|---|---|
| `mascot/mnemo.sply` | **Mnemo**, the Locora firefly, named after *mnemo*nics (and Mnemosyne, the Greek goddess of memory). It lights up a numbered route of loci (1 → 2 → 3), which is the method of loci in one picture. It hovers, flaps, glows, blinks, follows the pointer with its eyes and hops on click. |
| `brand/icon.sply` | App icon: a palace arch with a route of glowing loci and Mnemo's light. |

```bash
splinely-cli export mascot/mnemo.sply --output mascot/mnemo.svg   # interactive SVG
splinely-cli render mascot/mnemo.sply --output mascot/mnemo.gif   # animated preview
splinely-cli render brand/icon.sply --time 0 --output brand/icon.png
```
