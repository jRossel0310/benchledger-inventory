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
so apply never re-runs the provider chain — and writes every one in ONE
transaction: the value goes to its real column/attribute, and
`field_provenance` is upserted (`source`, `confidence = NULL` — a
user-approved field's trustworthiness is the `source` alone from that point
on, not a stale provider confidence number). `parts.metadata_complete` is
set (never cleared) once the preferred variant has a non-blank manufacturer,
mpn, and datasheet_url. A candidate that can't resolve (an unknown category
name, or a `variant.*` field with no preferred variant to write into) is
skipped and logged, not a hard failure — every other error aborts (and rolls
back) the whole apply.

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
`<DataLayout.cache>/digikey/<key>.json`, where `<key>` is the identity used
to look the part up (MPN if present, else supplier SKU), sanitized to
`[A-Z0-9_-]` and uppercased (`sanitize_cache_key` — filesystem-safe, and
case-insensitive so `"ne555p"` and `"NE555P"` share one entry). The cache is
checked **before** credentials are loaded: a part whose response was already
fetched still enriches even if credentials are later cleared, and a repeat
enrich/import never refetches over the network. A corrupt or unreadable
cache file is treated exactly like a cache miss (falls through to a live
fetch, or to `Ok(None)` if unconfigured) — it can never fail enrichment. The
cache holds only the raw product JSON, never a credential or the OAuth
token (which lives in memory only, on the `DigiKeyClient` instance, and is
refreshed on expiry). The cache is disposable — safe to delete entirely;
Phase 7's recovery mode is expected to offer clearing it.

## Provenance and review semantics

See `docs/schema.md`'s migration 0009 section for the `field_provenance`
table shape, and the "Compare-and-apply" section above for the
`requires_review` rule. The short version: `manual` is sacred (never
auto-overwritten), an `inferred` guess never silently replaces an existing
value, and every apply is caller-approved, key by key — there is no
"apply everything" path.

## Known limitations

See `docs/known-limitations.md`'s Enrichment (Phase 5c) section.
