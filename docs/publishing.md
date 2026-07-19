# Publishing the public snapshot

Phase 6 (spec §12/§13, public half): the desktop app exports a deterministic,
privacy-safe snapshot of the inventory and commits it to a public GitHub repo;
a static Vercel site (`apps/web`) renders it read-only. This doc covers how
the snapshot works, the one-time GitHub/Vercel setup, the close-time publish
flow, and troubleshooting.

## How the snapshot works

`inventory-sync::snapshot::build_snapshot` (`crates/inventory-sync/src/snapshot.rs`)
reads the database through the same public `Database` methods the app uses and
assembles a `Snapshot` struct tree that is deliberately a *narrower* view than
the schema — a field only exists on the snapshot structs if it is safe to
publish to a public repo and website.

**Deterministic canonical JSON.** `to_canonical_json` emits byte-stable
output: 2-space indent, LF line endings, exactly one trailing newline, stable
field order, every collection sorted (parts/projects/variants by ULID, bins
and tags by name). Building twice from the same DB yields identical bytes, so
the committed file never churns when nothing changed.

**Content digest excludes `published_at`.** `content_digest` is the SHA-256
of the canonical JSON *without* the `published_at` field (the only volatile
field). Publish compares this digest against `app_state.last_published_digest`
first: unchanged inventory ⇒ unchanged digest ⇒ **publish skipped with zero
network calls** — an untouched library never produces a new commit or a
Vercel redeploy.

**Exclusions are test-enforced.** The snapshot never contains:

- `private_notes` (public notes ARE included),
- prices or anything purchase-related — no `*_micros`, currency,
  `last_purchase_date`, or typical-order figures (a supplier listing keeps
  only supplier, SKU, product URL, and packaging),
- archived parts (absent entirely — name and id both),
- imports, invoices, order numbers, or attachment data,
- credentials, tokens, client IDs, or local filesystem paths,
- `field_provenance`,
- project `notes` and `repo_link` (name/status/description/build quantity and
  BOM part ids only),
- variant `notes` and dimension `notes` (internal working text).

`crates/inventory-sync/tests/snapshot.rs`'s exclusion test enforces this two
ways: a substring denylist over the serialized JSON (`private_notes`, `price`,
`micros`, `token`, `client_id`, `secret`, `import`, `invoice`, `attachment`,
`provenance`, `last_purchase`, `C:\`, `AppData`) AND a scan for planted
known-private values (a marker private note, variant note, dimension note, a
planted price's digits, an archived part's name and id, an imported order
number) seeded through the real `Database` API before building.

**Publish state** lives in the `app_state` table (migration 0010, see
`docs/schema.md`): `last_published_digest`, `last_published_at`, and the
`pending_publish` marker. The publish *config* is ordinary settings —
`publish_owner`, `publish_repo`, `publish_branch` (default `main`),
`publish_path` (default `apps/web/public/inventory.snapshot.json`),
`publish_vercel_url` (display-only).

**The publish call** (`inventory-sync::publish::publish_snapshot`): build →
digest → skip if unchanged; otherwise set the `pending_publish` marker
*before* uploading (so a crash or kill mid-upload still retries next launch),
render the publish form with a fresh `published_at`, GET the remote file's
sha, PUT via the GitHub Contents API (single-file update — nothing else in
the repo is touched), then record the new digest/timestamp and clear the
marker. Any failure leaves the marker set and surfaces a typed error.

## GitHub setup (one time)

1. **Create a PUBLIC repository** on GitHub (e.g. `you/inventory-site`).
   Everything committed to it is world-readable — which is fine, because the
   snapshot builder excludes anything private by construction (above).
2. **Generate a fine-grained personal access token** (GitHub → Settings →
   Developer settings → Fine-grained tokens): Repository access = *Only
   select repositories* → the repo from step 1; Permissions → Repository →
   **Contents: Read and write**. Nothing else. A token scoped this way can
   touch that one repo's files and nothing more.
3. **Configure the app**: Settings → Publishing → enter owner + repository
   (branch and snapshot path are optional — blank uses `main` and
   `apps/web/public/inventory.snapshot.json`), save; paste the token in the
   token form and save it. The token goes to Windows Credential Manager
   (entry `ElectronicsInventory-GitHub`) — never the database, exports,
   logs, or the snapshot itself. The form is write-only: the token is never
   displayed back, not even masked.
4. **Test connection** — a single read-only probe of the configured snapshot
   path. "connected" also covers a repo that doesn't have the snapshot file
   yet (first publish still ahead).
5. **Publish now** — the first publish creates the snapshot file in the repo.

## Vercel setup (one time)

1. Import the GitHub repo into Vercel (Add New → Project).
2. Set **Root Directory: `apps/web`** — `apps/web/vercel.json` is written
   relative to that root and supplies the rest:
   - `installCommand`: `pnpm install --frozen-lockfile` (pnpm resolves the
     workspace root from inside `apps/web`),
   - `buildCommand`: `pnpm --filter @ei/web build`,
   - `outputDirectory`: `dist`,
   - a rewrite of every path *except* `inventory.snapshot.json` to
     `/index.html`. The app uses hash routing (`#/…`), so the rewrite is
     belt-and-braces only; the snapshot path is excluded so a repo where
     nothing has been published yet returns a real 404 and the site shows
     "No snapshot published yet" instead of misreading the SPA fallback's
     HTML as an invalid snapshot.
3. Framework preset: **Vite** (auto-detected; the explicit build command in
   `vercel.json` wins either way).

After that, every publish commit auto-deploys: desktop **Publish now** →
snapshot committed to GitHub → Vercel redeploys → the site serves the new
`inventory.snapshot.json`. The digest skip means no-change publishes never
trigger a deploy. Optionally paste the deployed URL into Settings →
Publishing's Vercel URL field (display-only convenience).

## Publishing on close

Closing the desktop window runs a publish first (`close_flow.rs` +
`ClosePublishDialog.tsx`):

- Publishing **not configured** → the app just exits; no dialog.
- Otherwise a non-dismissable "Publishing before close…" dialog runs
  `publish_now`. Success — published *or* already up to date — exits
  immediately.
- Failure or a **20s timeout** → the dialog offers **Retry** and **Close
  anyway**. Close-anyway is safe: the pending marker was set before the
  upload started, so "Publish failed — it will retry next launch. Your local
  data is safe." is literally true on every path, including a timed-out
  attempt killed by the exit.
- **Quiet startup retry**: on the next launch the app retries a pending
  publish once, silently — success clears the pending state (the Dashboard
  card updates), failure stays quiet (the card keeps showing pending).
- **Wedged-frontend escape**: the Rust close guard re-emits the close event
  on repeat close requests, and if a close request arrives more than **30s**
  after the first one (`WEDGED_FRONTEND_GRACE`), it force-exits — a webview
  that can no longer run the dialog cannot trap the window open. The
  pre-upload pending marker keeps even that path lossless.

## Troubleshooting

"Test connection" answers with one of five fixed strings (never a response
body, HTTP detail, or the token):

| Result | Cause | Fix |
|---|---|---|
| `not configured` | Owner/repo not saved, or no token stored (a credential-store read failure reads the same) | Save owner + repo, then save a token, in Settings → Publishing |
| `connected` | Probe succeeded — the repo/branch is reachable (a missing snapshot file still counts: first publish ahead) | Nothing to fix |
| `rejected — check token` | GitHub returned 401/403: token invalid, expired, or missing Contents read/write on this repo | Regenerate the fine-grained PAT (Contents: Read and write, that repo selected) and re-save it |
| `repo or branch not found` | The configured repo/branch doesn't exist (surfaces on publish; the read probe can't always distinguish a missing repo from a missing file — see below) | Fix owner/repo/branch spelling; create the branch if needed |
| `network error or timeout` | Transport failure, rate limit, or an unexpected API response | Check connectivity; retry later |

Caveats worth knowing:

- **A wrong repo name can still read "connected"-adjacent on the probe.**
  GitHub's contents GET returns 404 both for "file not there yet" and "repo
  not visible to this token", and the client folds a GET 404 into "no file
  yet". The definitive check is the first **Publish now**, whose upload
  surfaces `repo or branch not found` distinctly.
- **Publish failed and the Dashboard shows "Publish pending — will retry on
  launch"**: that is the pending marker doing its job. Retry from Settings →
  Publishing (the pending row has a Retry button) or just relaunch.
- **The site shows "No snapshot published yet"** after setup: publish hasn't
  run yet, or Vercel deployed before the first publish commit — publish from
  the desktop, wait for the redeploy.
- **The site is stale**: check the repo — if the snapshot file's last commit
  is old, the desktop hasn't published since (remember: unchanged inventory
  skips publishing by design, so "stale" may just mean "nothing changed").

## Token storage

The GitHub token lives **only** in Windows Credential Manager (keyring entry
`ElectronicsInventory-GitHub`, same pattern as the DigiKey credentials). It
is never written to SQLite, settings, logs, exports, or the snapshot; the
commands layer is write-only (`set_github_token` / `clear_github_token` —
nothing ever returns it), errors carry fixed classification strings rather
than response bodies, and tests assert a planted token never appears in any
error's Display/Debug output or the Settings DOM.
