# Enrichment (`inventory-enrich` + `inventory-db::enrichment`)

Phase 5c's answer to spec §11 (enrichment) / §5 (provenance) / §16
(redaction), ADR #2/#3: given a part's identity (manufacturer, MPN, supplier
SKU, description, category), automatically propose values for its metadata
— always as a **reviewable diff**, never a silent write.

```
EnrichInput  →  run_chain([DigiKeyClient, DescriptionParser])  →  candidates
                                                                       ↓
                                          enrich_part_preview (diff vs. current + field_provenance)
                                                                       ↓
                                    caller approves a subset of keys
                                                                       ↓
                                         apply_enrichment (one transaction: write + upsert provenance)
```

## The provider chain

`crates/inventory-enrich` is a small, pure crate — no `rusqlite`, no
database access at all. Everything in it operates on `EnrichInput` (what's
known about a part: `manufacturer`, `mpn`, `supplier_sku`, `description`,
`category_hint`, every field optional) and produces `Enrichment` (a
`Vec<FieldCandidate>` plus discovered image URLs and free-text notes).

`trait EnrichmentProvider { fn enrich(&self, input: &EnrichInput) ->
Result<Option<Enrichment>, EnrichError>; }` — `Ok(None)` means the provider
ran fine but had nothing to add (not configured, no match, insufficient
input); `Err` is a real failure. Both are normal outcomes a chain tolerates.

`run_chain(providers, input)` runs an ordered list of providers and merges
their candidates **first-seen-key-wins**: put the most authoritative
provider first. Today's chain (`inventory-db::enrichment::
enrich_part_preview`) is `[&DigiKeyClient, &DescriptionParser]`:

1. **`DigiKeyClient`** (`crates/inventory-enrich/src/digikey.rs`) — the
   DigiKey Product Information V4 API. `source = digikey`, confidence 0.9.
   Contributes `description`, `category`, `variant.datasheet_url`,
   `variant.product_url`, `variant.lifecycle`, `variant.package`, and one
   `attr.<slug>` candidate per DigiKey `Parameters[]` entry (e.g.
   `attr.mounting_type`, `attr.frequency`), plus a product photo URL. Never
   fabricates: a DigiKey placeholder value (`"-"`, `"—"`, `"N/A"`) is
   filtered out, not surfaced as a candidate.
2. **`DescriptionParser`** (`crates/inventory-enrich/src/description.rs`) —
   always available, fully offline. Parses a DigiKey-style catalog
   description (`"RES 10K OHM 1% 1/4W 0603"`, `"CAP CER 0.1UF 50V X7R
   0603"`, `"IC OPAMP GP 2 CIRCUIT 8DIP"`) using `inventory-core`'s unit
   engine (`units::parse_with_kind`) and package normalizer
   (`packages::normalize_package`) to emit `category`, `variant.package`,
   and identity attributes (`attr.resistance`, `attr.tolerance`,
   `attr.power_rating`, `attr.capacitance`, `attr.voltage_rating`,
   `attr.dielectric`). `source = inferred`, confidence 0.5 — always below
   1.0, since a catalog description is shorthand, not a verified spec.
   Never guesses: an unparseable token is left alone (not read as a value
   just because it superficially fits the unit engine's grammar — e.g. a
   transistor MPN like `2N3904` is explicitly rejected as a resistance
   value), and two genuinely conflicting tokens for the same key drop that
   key entirely with an `"ambiguous <key>"` note rather than picking one.

Both providers key the same `field_key` scheme migration 0009's
`field_provenance.field_key` and the compare-and-apply layer use:
`variant.datasheet_url` / `variant.product_url` / `variant.lifecycle` /
`variant.package` (the part's preferred manufacturer variant), `description`
(`parts.description`), `category` (`parts.category_id`, by name), and
`attr.<attribute key>` (a category attribute, the same keys
`Database::set_attribute` accepts).

## Compare-and-apply

`Database::enrich_part_preview(part_id, cache_dir)`
(`crates/inventory-db/src/enrichment.rs`) builds the `EnrichInput` from the
part's current state, runs the chain, and diffs each candidate against:

- the part's **current value** for that key (read straight off
  `manufacturer_variants`/`part_attribute_values`/`parts` — dispatch mirrors
  the candidate's own `field_key` scheme), and
- that field's **current recorded provenance source**
  (`field_provenance.source`, if a row exists yet).

A candidate whose proposed value already matches the current value is
dropped — nothing to review. Everything else becomes a `FieldDiff{key,
current, proposed, source, current_source, requires_review}`.
`requires_review = true` when either:

- the field's current source is `manual` (a human typed it in
  deliberately), or
- the candidate's own source is `inferred` (the description parser) **and**
  the field already has a current value — a low-confidence guess must never
  silently replace something already there, confirmed by provenance or not.

**Nothing is written by preview.** `Database::apply_enrichment(part_id,
applied: &[AppliedField])` takes back only the caller-approved subset — each
`AppliedField` carries the value+source the caller already saw in the diff,
plus an `acknowledge_review` flag (see "Review enforcement" below) — so
apply never re-runs the provider chain — and writes every one in ONE
transaction: the value goes to its real column/attribute, and
`field_provenance` is upserted (`source`, `confidence = NULL` — a
user-approved field's trustworthiness is the `source` alone from that point
on, not a stale provider confidence number). `parts.metadata_complete` is
set (never cleared) once the preferred variant has a non-blank manufacturer,
mpn, and datasheet_url. A candidate that can't resolve (an unknown category
name, or a `variant.*` field with no preferred variant to write into) is
skipped and logged, not a hard failure — every other error aborts (and rolls
back) the whole apply.

### Review enforcement

`requires_review` is not merely advisory metadata for the UI to render — it
is enforced server-side. Immediately before writing each `AppliedField`,
`apply_enrichment_in_tx` re-derives, from the transaction's own current
state, whether that field is protected (the SAME rule `build_diff` used to
set `requires_review`, via one shared helper — `build_diff` and the
enforcement check call the identical function so the two can never drift
apart). If the field is protected and `AppliedField.acknowledge_review` is
not `true`, the whole apply is rejected with
`DbError::EnrichmentReviewRequired(field_key)` and rolled back — including
any other, individually-valid fields in the same call, since nothing
commits until every field in the batch has passed. `acknowledge_review`
defaults to `false` (`#[serde(default)]`), so a caller must set it
explicitly to overwrite a protected field; there is no way to opt out of
the check. This is defense-in-depth against a buggy or bulk caller writing
an `AppliedField` for a review-flagged diff it never actually surfaced to
(or got approval from) the user — the UI's confirmation step (5d) is the
primary safeguard, this is the backend's own guarantee that it cannot be
bypassed by a caller that skips or mishandles that step.

Re-running enrichment on a part that already has values is exactly what the
`requires_review` rule is for: DigiKey filling in a previously-blank
`datasheet_url` is a normal, no-review apply; DigiKey proposing a different
value for a `manual`-sourced attribute surfaces for approval instead of
overwriting it.

## Credentials: setup today (Settings UI arrives in 5d)

DigiKey credentials are never entered through the app UI in 5c — there is no
IPC command that accepts a secret. They're stored directly through
`inventory_core::secrets`, via the dev bin `set_digikey_credentials`
(`crates/inventory-core/src/bin/set_digikey_credentials.rs`), which is the
*same* module the app reads from, so it's guaranteed to write a value the
app can read back.

1. Register a free developer app at
   [developer.digikey.com](https://developer.digikey.com) — a **Production**
   app with the **Product Information V4** API scope enabled (see
   "Sandbox vs. production" below for why Production, not the sandbox app).
2. Set the two env vars for one shell session, then run the bin (PowerShell):

   ```powershell
   $env:DIGIKEY_CLIENT_ID = "..."
   $env:DIGIKEY_CLIENT_SECRET = "..."
   cargo run -p inventory-core --bin set_digikey_credentials
   Remove-Item Env:\DIGIKEY_CLIENT_ID, Env:\DIGIKEY_CLIENT_SECRET
   ```

   The env-var names are `DIGIKEY_CLIENT_ID` and `DIGIKEY_CLIENT_SECRET` —
   values are never accepted as command-line arguments (shell history/process
   listing exposure) and the bin never echoes them back, only their lengths
   (`"stored DigiKey credentials (client_id length=N, client_secret
   length=M)"`). Pass `--clear` to remove both stored entries.

Credentials land in Windows Credential Manager under the service name
`ElectronicsInventory-DigiKey`, two entries (`client_id`, `client_secret`).
`get_digikey_status` (a Tauri command) reports whether they're configured as
a plain `bool` — the secret itself never crosses the IPC boundary.

## Sandbox vs. production

`digikey_environment` is a non-secret `settings` value, `"sandbox"` |
`"production"`, defaulting to `sandbox` whenever unset or unrecognized
(`DigiKeyEnv::from_setting_str`) — a corrupted setting can never silently
start talking to the real API. The same credentials work against both
environments; only the base URL differs
(`https://sandbox-api.digikey.com` vs. `https://api.digikey.com`).

**Live finding:** DigiKey developer apps are registered for either sandbox
*or* production access, not both automatically. A live run against the
sandbox base URL with this project's registered (production) app credentials
returned HTTP 401 — the DigiKey API rejected the request credentials, mapped
to `EnrichError::Config`, which the chain logs as a note and degrades from
gracefully (per §11, a provider failure never blocks). Switching
`digikey_environment` to `production` and re-running against the same
credentials succeeded: a real lookup of Texas Instruments' NE555P returned a
full product record — description, category, datasheet URL, product URL,
lifecycle, package, and multiple parameter-derived attributes, a dozen
digikey-sourced candidate fields in total. A separate sandbox-only
credential pair (registered on a sandbox app) would be needed to exercise
the sandbox environment against this account; `sandbox` remains the default
regardless, since it's the safer failure mode for a fresh install with no
credentials configured yet.

## The cache

`DigiKeyClient` caches each raw API response to
`<DataLayout.cache>/digikey/<environment>/<key>.json`, where
`<environment>` is `"sandbox"` or `"production"` (`DigiKeyEnv::as_str`) and
`<key>` is the identity used to look the part up (MPN if present, else
supplier SKU), sanitized to `[A-Z0-9_-]` and uppercased (`sanitize_cache_key`
— filesystem-safe, and case-insensitive so `"ne555p"` and `"NE555P"` share
one entry). The cache is scoped by environment because sandbox and
production are different APIs that can return different data for the same
part number — without that scoping, a response cached under one environment
would be served verbatim after `digikey_environment` was flipped to the
other. The cache is checked **before** credentials are loaded: a part whose
response was already fetched still enriches even if credentials are later
cleared, and a repeat enrich/import never refetches over the network. A
corrupt or unreadable cache file is treated exactly like a cache miss (falls
through to a live fetch, or to `Ok(None)` if unconfigured) — it can never
fail enrichment. The cache holds only the raw product JSON, never a
credential or the OAuth token (which lives in memory only, on the
`DigiKeyClient` instance, and is refreshed on expiry). The cache is
disposable — safe to delete entirely; Phase 7's recovery mode is expected to
offer clearing it.

## HTTP timeouts

`DigiKeyClient`'s underlying `reqwest::blocking::Client` is built with a
15-second total request timeout and a 5-second connect timeout. Every call
this client makes (the OAuth token request, the product-details lookup)
runs synchronously inside a Tauri command handler, so an unbounded timeout
would let a hung or slow-loris DigiKey endpoint hang that command — and the
UI thread waiting on it — indefinitely. A timeout expiring surfaces as
`EnrichError::Network`, the same variant a connection failure already maps
to, so the chain degrades from it exactly like any other transport failure
(logged as a note, the rest of the chain still runs).

## Provenance and review semantics

See `docs/schema.md`'s migration 0009 section for the `field_provenance`
table shape, and the "Compare-and-apply" / "Review enforcement" sections
above for the `requires_review` rule and how it's enforced. The short
version: a `manual`-sourced field, or an existing value about to be
replaced by an `inferred` guess, requires an explicit per-field
`acknowledge_review` to overwrite — enforced in `apply_enrichment_in_tx`
itself, not just flagged by `build_diff` for the UI to (hopefully) respect
— and every apply is caller-approved, key by key; there is no "apply
everything" path.

## UI (Phase 5d)

`apps/desktop/src/features/part/EnrichmentDiffDialog.tsx` and
`apps/desktop/src/features/settings/DigiKeySettings.tsx` are the human side
of everything above — the compare-and-apply diff and the credentials/
environment controls, respectively. See `docs/ui.md`'s "Enrichment diff
dialog" and "Settings" sections for where each is reached from; this section
covers what each control actually does.

### UI: the diff dialog

`EnrichmentDiffDialog` is opened only from `PartDetail`'s "Refresh product
data" button — never mounted eagerly — so `enrich_part_preview`'s DigiKey
call never fires just because a part-detail screen is open
(`useEnrichmentPreview(partId, {enabled: true})` only takes effect once the
dialog itself exists). It fetches once per open and renders every
`FieldDiff` as a current -> proposed row with a "Discovered images" strip
above the list when the preview returned any.

- **Every row gets an `include` checkbox.** For an unprotected row
  (`requires_review: false`), checking it is enough — `Apply` sends it with
  `acknowledge_review: false`.
- **A protected row (`requires_review: true`) needs a second, explicit
  confirmation checkbox before `include` actually arms it** — this is the
  UI mirror of the backend's own enforcement (`apply_enrichment_in_tx`
  rejects an unacknowledged protected field with a typed
  `EnrichmentReviewRequired` regardless of what the UI sends; see this
  doc's "Review enforcement" section above). The two confirmation labels
  match the same two triggers `is_protected_field` checks, in the same
  priority order:
  - **"Overwrite manually-set value"** — shown when the field's *current*
    source is `manual` (a human typed it in deliberately).
  - **"Accept inferred over existing"** — shown when the current source is
    anything else but the *candidate's* own source is `inferred` (the
    low-confidence description parser) and a current value already exists.
  A row that's `include`d but not (yet) confirmed is simply left out of the
  apply payload, with an inline hint explaining why — it never silently
  arms itself, and it never blocks any other row's apply. Unchecking
  `include` also clears that row's confirmation, so re-checking it later
  requires re-confirming rather than resurrecting a stale approval.
- **"Select all" only arms unprotected rows.** It's rendered only when at
  least one unprotected row exists, and it never touches a protected row's
  `include` or confirmation — a bulk control silently setting
  `acknowledge_review: true` on a human's behalf is exactly what the trust
  rule exists to prevent (spec's Global Constraints: "never by bulk
  select-all").
- **Images are display-only.** A non-empty `images` list renders as a
  thumbnail strip above the field rows for visual reference — there is no
  checkbox, no apply action, and no attachment created from an image today;
  a broken image URL degrades silently (removed from the strip, not shown
  as a broken-image icon). Turning a discovered image into a real
  `attachments` row is not implemented (see `docs/known-limitations.md`).
- **No diffs** (every candidate already matches the part's current values)
  renders a plain "No differences" status instead of an empty list — Apply
  stays disabled since there's nothing armed.
- **`EnrichmentReviewRequired`** (a genuine race — e.g. someone else edited
  the part concurrently) surfaces as a plain message and keeps the dialog
  open with nothing applied, rather than closing on a partial success.

### UI: Settings

`DigiKeySettings` (`/settings`) is the only UI surface that ever collects a
DigiKey secret, in four pieces:

- **Status** — `configured` (bool) and `environment`, read-only; there is no
  masked `••••` placeholder for a stored credential, since that would fake a
  value the backend never actually returns.
- **Credentials: save / replace / remove** — one form (Client ID as plain
  text, Client Secret as `type="password"`) that doubles as "Replace" once
  already configured (the submit button's own label says which). Saving is
  write-only: `set_digikey_credentials` returns nothing, and the component
  clears both fields from local state the instant the mutation succeeds —
  nothing downstream ever holds the values again, no draft persists across
  a re-render or an unmount. "Remove" (only shown once configured) opens a
  confirm dialog before clearing both stored entries from Windows Credential
  Manager; enrichment then falls back to description parsing alone.
- **Environment toggle** — sandbox/production radios, `useSetDigiKeyEnvironment`
  on change. Switching resets any on-screen "Test connection" result, since
  a result from before the switch is now against the wrong environment and
  would otherwise read as current when it's stale.
- **Test connection** — an OAuth2 token-fetch probe only (never a product
  lookup), surfacing one of the backend's own fixed strings: "Connected
  (Production)"/"Connected (Sandbox)" on success, or a plain rejection
  message — never a raw response body or anything credential-shaped.

## Known limitations

See `docs/known-limitations.md`'s Enrichment (Phase 5c) section.
