# Locora plugin SDK

Plugins are WASI components (`wasm32-wasip2`). `#[locora_plugin]` generates the
manifest and component exports. Import `locora_sdk::prelude::*` for the typed API.
The host and plugins currently speak API/WIT **0.6**; rebuild older components.
0.3 introduced filter/call imports and the filter export; 0.4 adds answer fields
to palace DTOs, changing their Postcard layout. 0.5 adds `Answer.accept`
(further Guess answers) and `note` to palace loci – again a new layout. 0.6 adds the locus
picture (`image`, a media hash, and `image_size`) and `zoom` to `Locus`,
`NewLocus` and `LocusPatch`. JSON defaults do not make that binary layout compatible. Neither 0.2–0.5 components can simply be relabelled.

## Host-driven filter chains

```rust
use locora_sdk::prelude::*;

#[locora_plugin(name = "Punkte")]
struct Points;

impl Plugin for Points {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.filter::<JourneyScore>(10, |_, mut score| {
            score.points = score.points.saturating_add(1);
            Ok(score)
        })
    }
}
```

A hook implements `locora_api::Hook` with `NAME` and a serializable `Value`.
`core::JourneyScore` uses `core::Score { palace_id, locus_id, points: u64 }` and
names `core.journey.score`. These are extension points, not scoring rules.

The native caller invokes `host.filter::<JourneyScore>(score)`. Each handler
receives its predecessor's successful result. Handlers run in ascending
`(priority: i32, plugin id)` order, independent of load order. One registration
per hook/plugin is allowed; duplicates return an error without replacing the
previous handler or priority. Unload removes registrations; reload starts fresh.
Unknown hook names are allowed for custom typed hooks. With no handlers, the
value is unchanged. Plugins cannot synchronously invoke chains or other plugins.

The host checks every response against the hook's value type. An ordinary error
or undecodable response generates `HostEvent::HandlerError`, keeps the previous
value and leaves the plugin active. A trap (including panic or exhausted fuel)
disables only that plugin; later handlers still run. Published events are queued
until the whole chain completes, avoiding guest re-entry. Already committed
storage writes are **not rolled back** on handler failure. A chain snapshots its
registrations at entry; new registrations apply to the next invocation.

The CLI now calls this hook once per evaluated answer, with Core's base points:
correct = 1, incorrect = 0. Only the resulting `points` field is used; palace and
locus identities come from the trusted route. Points totals saturate at `u64::MAX`.
Filters cannot change correctness or the review grade. No XP, combos, trophies,
or persistent points ledger is implemented.

## Core journeys

`plugins/core` is a real WASM component embedded alongside `review` in the CLI.
It provides `journey::Evaluate` (`core.journey.evaluate@1`), called by the native
client as `host.call_plugin::<Evaluate>("core", &EvaluateRequest { answer,
expected, submission, elapsed_ms })`. This provider-bound call never falls back
to a competing service. Profile loading cannot replace an already-loaded plugin
identity. `core`/`review` remain reserved even if the embedded component is
missing; explicit development hot reload remains available through `load_file`. The SDK prelude exports these types, `Evaluation`,
`Answer`, `AnswerKind`, `QuizOption`, `Submission` and `Scored`.

- Recall: `Submission::Recall(Grade)` preserves all four self-ratings; Hard,
  Good and Easy count as correct. Existing terminal reveal/rating controls and
  post-reveal workbook actions remain unchanged.
- Guess: `Submission::Guess(text)` compares trimmed, Unicode-lowercased whole
  strings against `expected` (the item binding resolved with `View.reveal=true`
  at submission). No fuzzy, accent or internal-whitespace normalization.
  Blank submissions/expected text are invalid, not correct empty matches.
- Quiz: `Submission::Quiz(Vec<u32>)` selects zero-based option indices. The set
  must contain every correct option and no incorrect one. Partial answers fail;
  duplicates and out-of-range indices are errors. Option order is authored,
  not shuffled. CLI entry uses one-based comma-separated numbers.
- `Submission::Forfeit` is always incorrect/Again. In automatic questions,
  Enter or a workbook Reveal forfeits. When workbook actions exist, automatic
  feedback stays on the revealed locus: `?`/`!name` act there, Enter advances,
  and q/EOF stops. Feedback actions never grade again; the submitted answer is
  already recorded, even if quit follows. Without actions feedback advances
  directly. Quit/EOF at an unanswered prompt does not grade.
- Incorrect = Again. Correct automatic answers at **<= 5,000 ms** = Easy,
  otherwise Good. This is an explicit initial Locora policy, not legacy
  gamification timing. Time starts at the prompt and ends at submission,
  including commands and invalid-entry retries; Recall includes self-rating.

Core owns evaluation/grade/base points. The native client owns route snapshots,
terminal I/O, workbook state, timing and hook orchestration. It publishes the
unchanged `core::LocusAnswered` to Review/FSRS and then `journey::Scored`
(`core.journey.scored`) with final points. `JourneyCompleted` is emitted only
for a nonempty, fully answered selected route per palace; partial runs still
schedule their answered loci. Core does not validate a replay transcript:
forfeiture and resolved expected answers are trusted native-client inputs.

The CLI refuses practice/review before prompting if Core is unavailable. Builds
with `LOCORA_SKIP_PLUGINS=1` still support editing/transfer/development; rebuild
with `wasm32-wasip2` to practise. Core service
failures are explicit, with no native fallback evaluator.

`locora edit P 1 --answer '{"type":"guess"}'` changes a locus; use
`{"type":"recall"}` to reset it, or Quiz as shown in the
[format guide](../../format/README.md). New/old loci default to Recall. Guess
uses the resolved item plus `accept` (`--accept TEXT`, repeatable), compared with
`normalize` (case, punctuation, spaces ignored); Quiz uses explicit
correct/incorrect options, independent of the item (shown as explanation), and
the client shows `Answer::quiz_selection(seed)`: every correct option plus three
random wrong ones. Indices in `Submission::Quiz` always refer to `options`. `q`, `?` and `!name` are controls; prefix a
Guess with `=` for literal input, e.g. `=q`, `=?`, `=!name`, `==value`.
Broken/empty Guess expected values are diagnosed and left ungraded, while a
conscious Forfeit can still record Again. No hidden binding is evaluated just
to display the prompt; invalid quiz number entry retries without scheduling.

SQLite migration 4 adds answer JSON without changing routes, bindings or Review
KV. Store/palace DTOs include `answer`; patch `None` preserves it and
`Some(Answer::default())` resets to Recall. Answer validation occurs before any
write/op-log change. Read snapshots expose authored answers, not resolved item
bindings. Archives retain version 1 for Recall-only exports and use version 2
for Quiz/Guess; see the format guide. There is no legacy mnemodim comma-answer
parser, automatic distractor generation or additional answer editor UI.

## Palace host service

```rust
use locora_sdk::prelude::*;

#[locora_plugin(name = "Paläste", permissions = [palaces_read, palaces_write])]
struct PalacesPlugin;

impl Plugin for PalacesPlugin {
    fn activate(ctx: &mut Ctx) -> Result {
        let created = ctx.palaces(&palaces::Request::Create {
            name: "Garten".into(),
        })?;
        if let palaces::Response::Created { id, .. } = created {
            let _snapshot = ctx.palaces(&palaces::Request::Get { query: id })?;
        }
        Ok(())
    }
}
```

`ctx.palaces(&request)` is shorthand for
`ctx.call_host::<palaces::Palaces>(&request)`. The reserved native service is
`core.palaces@1`. Authorization is enforced **inside the host's WIT import**,
not by SDK checks, including for plugins that call WIT directly.

| Request | Required capability | Response |
| --- | --- | --- |
| `List` | `palaces:read` | `List(Vec<Summary>)` |
| `Get { query }` | `palaces:read` | `Palace(Palace)` |
| `Create { name }` | `palaces:write` | `Created { id, name }` |
| `Rename { id, name }` | `palaces:write` | `Done` |
| `Delete { id }` | `palaces:write` | `Deleted(bool)` |
| `AddRoom { palace_id, name, kind, media }` | `palaces:write` | `Room(Room)` |
| `AddLocus { room_id, locus }` | `palaces:write` | `Locus(Locus)` |
| `UpdateLocus { id, patch }` | `palaces:write` | `Done` |
| `DeleteLocus { id }` | `palaces:write` | `Deleted(bool)` |

Write does **not** imply read. Update returns no existing fields, even for an
empty patch. Create/add return only newly created objects. Read requests return
ordered route snapshots, not evaluated workbook bindings, shared tables, media
bytes, review state or filesystem paths. `Get` accepts an exact id,
case-insensitive name or unique id prefix of at least four characters.

Coordinates must be finite and in `[0, 1]`. Video timestamps must be finite and
nonnegative. In `LocusPatch`, `time: None` preserves the timestamp,
`Some(None)` clears it and `Some(Some(seconds))` sets it. Other absent patch
fields are preserved. Room media must reference an **existing media hash**,
never a path or URL; blank rooms cannot have media. Image/video rooms require
media. Mutations use the store's existing validation, IDs, route order and op
log; deletion cascades as in native operations. Missing deletion targets return
`Deleted(false)`; other missing references and invalid requests return errors.

Successful creates, palace deletes and locus deletes queue the existing
`PalaceCreated`, `PalaceDeleted` and `LocusDeleted` events with **host** origin.
They reach subscribers (including the calling plugin) after the current guest
call/chain, never by synchronous re-entry. Committed mutations still notify
other plugins if activation fails or during deactivation. Native service calls
also dispatch these events. In particular, `review` forgets deleted state.
Guest-published events retain their existing self-exclusion behavior. The plugin
id `host` is reserved. Capabilities gate service access, **not the public event
bus**; subscriptions are not a confidentiality boundary.

`PluginHost::with_palaces(cache, kv, Arc<locora_store::Store>)` binds a backend
before instantiation. Start functions and the manifest probe have no capabilities;
declared permissions are granted only after successful instantiation, before
activation. Native `host.call::<palaces::Palaces>(...)` callers are trusted and do
not need guest capabilities. `new`/`with_kv` remain usable without
a palace backend; calls then fail explicitly. Plugins cannot override the native
service with `provide`, even when its backend is absent. `provides` reports the
palace service only when a backend is attached. The reserved `ui.highlight@1`
service accepts short plain-text tour steps with stable element IDs and requires
`ui:highlight`; a guest event cannot bypass this check.

The CLI application attaches the selected profile's persistent store. In
contrast, `plugin dev/run` use isolated in-memory palace and plugin storage.
`plugin check/install/ls` inspect metadata without activation or profile backends;
`plugin remove FILE.wasm` deletes files without executing any guest code;
removal by ID uses the same isolated metadata inspection.

## Local installation and consent

`locora plugin install FILE.wasm` reviews the manifest/API and displays a short
German warning, identity and requested permissions. Interactive consent defaults
to **cancel**; EOF/anything except `j`/`ja` refuses. Non-terminal stdin always
requires explicit `--yes` (piped `ja` is not consent). Even `--yes` displays the
warning and cannot bypass validation. Supported permissions are exactly
`storage`, `palaces:read`, `palaces:write`, `ui:highlight`; SDK `notify` is recognized but not yet
implemented and is refused, as are `network` and unknown permissions.

Installation stores the reviewed bytes in `<data>/plugins/<id>.wasm` and the
approved manifest and SHA-256 of those exact bytes in `<id>.wasm.json`, with
atomic no-clobber publication (approval first, component last). IDs are 1–128
ASCII lowercase letters/digits/`-`/`_`,
starting with a letter/digit and excluding Windows device names; filenames never
come from display names or paths. Components/approval must be regular files, not
symlinks or pipes; removal unlinks a component/approval symlink without following
its target. The profile's `plugins` entry must be a real directory, not a symlink;
this is checked before opening the profile database.
Existing components, destinations and duplicate manifest IDs under other filenames
are refused. Broken unrelated files do not block installation. Packages,
a registry and network downloads are not implemented.

`plugin sign FILE.wasm --key KEY` creates `FILE.wasm.sig`, a JSON signature
file containing the Ed25519 public key and a signature over the exact component
bytes. `plugin keygen KEY` creates a private signing key; keep it secret.
`plugin trust PUBLIC_KEY --name NAME` records a publisher in the profile's
`plugin-publishers.json` trust store. Installation verifies any supplied signature
and records its signer in the approval receipt; an unknown signer requires consent.

Installation does **not** activate even after approval; activation is deferred to
the next normal profile command. `PluginHost::inspect_bytes` instantiates with no
capabilities, no profile backends and no inherited WASI resources; start/manifest
code still runs, with fuel, and its events/logs are discarded. It does not write
the compiled cache. Profile loading uses `load_approved_file`, the same duplicate-
rejecting policy as `load_additional_file`, plus an exact approved-manifest and
byte-hash check before granting capabilities/activation. The byte hash is
checked before compiling or probing a component. This prevents nondeterministic manifests
from gaining rights not displayed at install. Missing/invalid approvals refuse
activation; manually copied files must go through `plugin install` instead. Legacy
manifest-only approvals also require removal and a fresh installation.

`plugin ls` reports built-ins and installed metadata (not activation/health),
including broken/incompatible files and missing/mismatched approvals. `plugin
remove ID` resolves manifest identities even under renamed files, refusing
ambiguous identities rather than deleting an unrelated filename. Explicit
`plugin remove FILE.wasm` bypasses probes, so broken or trapping components remain
removable. A broken canonical `ID.wasm` or orphan approval also permits ID-based
cleanup. Only profile files/approval are removed; palace data and plugin KV
survive. Embedded Core/Review remain protected, even in builds without them;
extraneous profile files named `core.wasm`/`review.wasm` can be cleaned up. An
interrupted installation may leave an orphan `.wasm.json`; remove its ID before
retrying.

`plugin update FILE.wasm` requires an installation approved with a signature,
a valid signature from the same signer, and a numerically greater dotted version
(e.g. `1.2.4` after `1.2.3`). It checks the installed component against its approval
before replacing it, displays permission changes, and asks for consent again
(or requires `--yes` without a terminal). Unsigned installations cannot be updated
this way.

The signature authenticates the component bytes; the approval receipt records
local consent and the signer. The profile directory, approval receipts and trust
store are trusted local user data; a local process with write access can change
them. Fuel is not a memory, compilation-time or host-buffer quota: inspection is not a resource-exhaustion
boundary for hostile WASM. Development `dev/run` deliberately execute untrusted
code in ephemeral storage and are not installation approval.

This service deliberately excludes workbook editing, media import, room
removal/reordering and bulk transactions. Calls are synchronous; host database
work is not metered by WASM fuel. Existing fuel isolation and event cascade
limits remain in force, but no new payload/memory quotas are introduced.

## Checks

From the repository root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
(cd plugins && cargo clippy --workspace --all-targets -- -D warnings)
(cd plugins && cargo fmt --all -- --check)
```

`crates/locora-host/tests/plugins.rs` builds the real WASM fixtures (including
read-only, write-only, malformed and lifecycle components). Tests cover ordering,
reload/unload, duplicate registration, normal errors, invalid responses,
panics/fuel, deferred events, raw-WIT capability denials, CRUD/persistence,
validation, write-only update privacy and deletion notifications. Core tests
exercise real evaluation, timing boundaries, multiple correct choices, lifecycle,
points filters and Review independence; `apps/cli/tests/journey.rs` covers mixed
practice/review, forfeits/quit, literal controls, bindings, broken answers and
archive round trips. To run the plugin workspace's pure unit tests natively:

```sh
(cd plugins && cargo test --workspace --target x86_64-unknown-linux-gnu)
```

Use your native Rust target on other platforms.
