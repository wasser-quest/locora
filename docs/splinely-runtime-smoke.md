# Splinely runtime smoke test

## Scope

Checked the current debug app on Linux/WebKitGTK with a freshly built Trunk UI and a separate profile at `/tmp/locora-native-runtime-profile`. No existing user profile was opened.

Commands:

```sh
(cd apps/app/ui && trunk build)
cargo build --manifest-path apps/app/Cargo.toml -p locora-app
# Debug builds require the frontend at localhost:1420.
python3 -m http.server 1420 --bind 127.0.0.1 --directory apps/app/ui/dist
LOCORA_DATA=/tmp/locora-native-runtime-profile apps/app/target/debug/locora-app
```

## Observations

- REA process capture observed the app starting its WebKit network and web processes. The capture stopped the app by SIGTERM at its requested 18-second deadline; this was not an application crash.
- REA Evidence: `ev_6beea713a9386ad2e949f53c55cf7c6c396ebf047dea90eb4367627794c2824d`.
- REA process capture does **not** observe rendering, DOM state, network traffic or animation. Its native accessibility observation provider is Mach-O-specific; its browser tools expect CDP, not WebKitGTK's inspector protocol.
- In a separate native-window check of the same build, Simo appeared on the welcome screen. Two screenshots showed changed pixels in the Simo region, including the figure and firefly.
- Started the tutorial, started its route and revealed the first answer. Simo appeared at the first locus. Two further screenshots showed changes within that revealed locus image.
- No runtime-start failure alert was visible in those checked views.
- An isolated WebKit probe of the generated Simo document also returned `playing: true`, advancing autoplay clips (`breathe`, `sway`, `hover`, `flap`, `glow`, `loci`) and no runtime diagnostics.

## Pointer-follow regression

Runtime playback and cursor following are separate. App frames intentionally use
`pointer-events: none` so animations cannot steal clicks/focus. Without a bridge,
Splinely's pointer followers therefore receive no cursor input.

The app now forwards mouse coordinates into the sandboxed frame, accounting for
frame position and CSS scale. The bridge accepts parent-window messages only and
dispatches pointer movement to the existing Splinely runtime; it does not replace
its follower logic. Window blur clears the pointer. Native frame pointer events
remain disabled.

REA-controlled WebKit probe of the generated Simo document:
`ev_4934eb32c87ef65819a58cfafc186dd9699a48fc7964ea9025743fadbbfd41e0`.
It paused the timeline, sent left/right pointer messages and ticked the runtime.
`pupil_l` moved from x `212.01996397312564` to `217.9934669308472`;
`runtimeReady` was true and the probe exited with code 0. This verifies the bridge
and actual follower response in isolated WebKit, not every app placement or every
asset. Full native-app pointer mapping still needs visual checks.

## Limits

The reported static Simo/tutorial behavior was not reproduced in this build. This does not establish that every installed build or every tutorial locus animates correctly. An outdated frontend is a possible explanation, not a confirmed cause. The generated SVG is a document containing the original embedded `.sply` project and Splinely JavaScript runtime, not a raster conversion or evidence of static rendering by itself.

Temporary screenshot files and test profiles are intentionally not committed. To repeat the check, compare the welcome Simo and the revealed first tutorial locus at multiple times, and check for the runtime-start alert. Remaining checks include packaged release builds, all tutorial loci, other platforms and reduced-motion behavior.
