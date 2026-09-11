# Mod catalog / downloader — FACTS

Research date: **2026-09-11**. Target: a Prism-Launcher-style mod browser (Modrinth +
CurseForge) with installed-mod management, for Nebula (Rust + Slint).

**Prism Launcher is GPL-3.0.** Nothing here is copied code. Everything below is
observed behaviour, endpoints, parameters and data formats, described in prose so it can
be re-implemented cleanly under MIT.

## Citation convention

Every fact carries one of these tags plus a URL:

| Tag | Meaning |
| --- | --- |
| `[docs]` | Stated in official API documentation |
| `[live]` | Observed in an actual API response on 2026-09-11 |
| `[prism]` | Read from PrismLauncher source on GitHub |
| `[issue]` | PrismLauncher / upstream issue tracker |
| `[repo]` | Already present in this repository |
| `UNVERIFIED` | Could not be confirmed against a primary source — do not build on it |

Where `[docs]` and `[prism]`/`[live]` disagree, both are stated and the conflict is
called out explicitly.

---

# A. Modrinth API v2

Base URL `https://api.modrinth.com`, staging `https://staging-api.modrinth.com`.
`[docs]` https://docs.modrinth.com/api/

All v2 paths below are prefixed `/v2`, e.g. `https://api.modrinth.com/v2/search`.

## A.1 Rate limit, User-Agent, auth

| Fact | Value | Source |
| --- | --- | --- |
| Rate limit | **300 requests / minute / IP** | `[docs]` https://docs.modrinth.com/api/ |
| Header | `X-Ratelimit-Limit` — max requests in the window | `[docs]` same |
| Header | `X-Ratelimit-Remaining` — requests left in window | `[docs]` same |
| Header | `X-Ratelimit-Reset` — **seconds** until window resets | `[docs]` same |
| Over limit | HTTP **429**, body `{"error":"ratelimit_error","description":"rate limit exceeded; retry after N milliseconds"}`, and the 429 itself carries `X-Ratelimit-*` with `remaining: 0` | `[source]` labrinth `routes/mod.rs` (`ApiError::RateLimit -> TOO_MANY_REQUESTS`) + `util/ratelimit.rs` `[corrected 2026-09-11: 3 refuters; the docs page at https://docs.modrinth.com/api/ never mentions 429 — the citation was wrong, the value was right]` |

**User-Agent is required to be uniquely identifying.** Modrinth's own worked example
ladder `[docs]` https://docs.modrinth.com/api/ :

```
bad     User-Agent: okhttp/4.9.3
ok      User-Agent: project_name
better  User-Agent: github_username/project_name/1.56.0
best    User-Agent: github_username/project_name/1.56.0 (launcher.com)
        (or with a contact email)
```

Recommended for us: `dscalzi/Nebula/1.0.0 (+https://github.com/dscalzi/Nebula)`.
This repo currently sends `nebula-rust/{CARGO_PKG_VERSION}` `[repo]`
`crates/nebula-core/src/remote.rs:27` — that is on the "good" rung, not "best"; it
should gain a contact URL before hammering Modrinth.

**But it is a policy, not a technical gate.** `[corrected 2026-09-11: 2 refuters, live probe + labrinth
source]` A request with the `User-Agent` header removed entirely (`curl -H "User-Agent:"`,
verified with `-v` that none was sent) returns **HTTP 200** with normal rate-limit headers.
The rate-limit middleware keys on the client IP (`CF-Connecting-IP` or peer address), not on
the UA; labrinth only *logs* the UA (`main.rs`). Enforcement is a manual blocklist
("increases the likelihood that we will block your traffic"). Send a good UA anyway — just do
not design around it being enforced. (Trivia: the docs label the second rung **"Good"**, not
"ok".)

Live header values on an ordinary 200 `[live 2026-09-11]`
`GET https://api.modrinth.com/v2/project/sodium`:

```
X-Ratelimit-Limit: 300
X-Ratelimit-Remaining: 300
X-Ratelimit-Reset: 0
```

Auth: personal access tokens or OAuth2 via the `Authorization` header. **Read endpoints
used by a mod browser need no token.** `[docs]` https://docs.modrinth.com/api/

**Deprecation warning:** the docs state that when v3 becomes generally available, v2
will be deprecated and eventually return permanent **HTTP 410**. `[docs]`
https://docs.modrinth.com/api/ — build the client so the version prefix is one constant.

## A.2 Search — `GET /v2/search`

`[docs]` https://docs.modrinth.com/api/operations/searchprojects/

| Param | Type | Default | Limit | Notes |
| --- | --- | --- | --- | --- |
| `query` | string | — | — | free-text search term |
| `facets` | string (JSON) | — | — | see below |
| `index` | enum | `relevance` | — | `relevance` \| `downloads` \| `follows` \| `newest` \| `updated`. The server *also* accepts the **undocumented aliases** `date_modified` (== `updated`) and `date_created` (== `newest`), plus server-only `minecraft_java_server.*` values; anything else → HTTP 400 `invalid index '<x>'` `[corrected 2026-09-11: labrinth `search/backend/common.rs` `parse_search_index` + live probe]` |
| `offset` | integer | `0` | — | results to skip. **The echoed top-level `offset` is rounded DOWN to a multiple of `limit`** (labrinth converts to page paging: `page = offset / hits_per_page + 1`, and the v2 response recomputes `offset = (page-1) * limit`). Live: `limit=3&offset=7` → response `offset: 6`; `limit=3&offset=10` → `9`; `offset=3` at the default `limit=10` → `0`. **Never drive pagination from the echoed value — keep `offset = n * limit`.** Negative offset → HTTP 400 `invalid offset` `[corrected 2026-09-11: 2 refuters, live probes + labrinth `search/backend/common.rs`]` |
| `limit` | integer | `10` | **max 100** | over-max is **clamped, not rejected** (`.min(100)` in `search/backend/common.rs`) — `limit=101` returned 200 OK with 100 hits `[live]`. Edge cases: **`limit=0` returns 1 hit and echoes `limit: 1`** (`hits_per_page = if limit == 0 { 1 } else { limit }`) even though the OpenAPI schema says `minimum: 0`; a non-numeric `limit` → HTTP 400 `invalid limit` `[corrected 2026-09-11: labrinth source + live probe]` |

**DISPUTED: does `offset` skip exactly N results, or does the skip itself snap?**
Two refuters probed this and reached different conclusions, both with live data and source
citations. **Position A** (labrinth `search/backend/common.rs`: `page = offset /
hits_per_page + 1`): *only offsets that are a multiple of `limit` skip exactly that many
results* — the paging is page-based end to end, so the hits snap too. **Position B**
(labrinth `search/backend/elasticsearch/mod.rs` queries `from: parsed.offset` verbatim; live
`index=downloads&query=sodium&limit=3`: `offset=0` → `[sodium, sodium-extra,
reeses-sodium-options]`, `offset=1` → `[sodium-extra, reeses-sodium-options, indium]`):
*the hits skip exactly `offset`; only the echoed top-level `offset` snaps down.* Position B
has direct evidence that a non-multiple offset shifts the window by one, which Position A's
model cannot produce; Position A may be reading a different (typesense) backend path.
**Both positions imply the same client rule, so the disagreement is not blocking: page with
`offset = n * limit` and never read the echoed `offset` back.**

### Facet syntax

A facet is `{type}{operation}{value}`. Operations: `:` (equals), `!=`, `>=`, `>`, `<=`,
`<`. `[docs]` https://docs.modrinth.com/api/operations/searchprojects/

**`=` is a first-class synonym of `:`.** `[corrected 2026-09-11: live probe + labrinth
`routes/v2/projects.rs` `parse_facet` matching `':' | '='`, and `search/filter/legacy_v2.rs`
`parse_condition` rewriting `a:b` to `a = b`]` Live: `[["project_type=mod"]]` and
`[["project_type:mod"]]` both return the same `total_hits`. Use `:`; just do not be surprised
by `=` in someone else's URL.

The value is a **JSON array of arrays**, URL-encoded:

* inner array → **OR**
* outer array → **AND**

```json
[["project_type:mod"],
 ["categories:fabric","categories:forge","categories:neoforge"],
 ["versions:1.20.1"]]
```

Verified live: `GET /v2/search?query=sodium&limit=1&facets=%5B%5B%22project_type%3Amod%22%5D%5D`
returns hits. `[live]`

**Facet loaders under `categories:` — but `loaders:` exists too.** `[corrected 2026-09-11: 3 refuters,
live probes + labrinth `routes/v2/projects.rs` and `search/backend/typesense/mod.rs`]`
The earlier claim "there is no `loaders` facet" was **wrong**. The v2 handler rewrites only
`versions`→`game_versions`, `project_type`→`project_types` and `title`→`name`, and passes
**every other facet key through unchanged** (`x => x`) to the v3 index, which has a real
`loaders` field. Live 2026-09-11, `query=sodium`:

| Facet | `total_hits` |
| --- | --- |
| *(none)* | 348 |
| `[["categories:fabric"]]` | 240 |
| `[["loaders:fabric"]]` | 80 |
| `[["loaders:forge"]]` | 30 |
| `[["loaders:neoforge"]]` | 45 |
| `[["bogusfield:x"]]` | 0 (unknown facet types are **not** rejected — they silently match nothing) |

The two are genuinely different filters: `categories` is the **legacy project-level** field
that matches either a project category *or* a loader, while `loaders` is the **version-level**
field, so it is the stricter one. For modpacks `loaders:fabric` returns 0 (modpacks index as
`loaders:mrpack`) while `categories:fabric` returns 159. **Keep using `categories:`** — it is
documented and it also covers modpacks — but do not assume `loaders:` errors out; it does not,
and mixing the two silently ANDs two different filters.

The facet type list is `project_type`, `all_project_types`, `categories`, `versions`,
`open_source`, `environment`, `disclosure_types`, `title`, `author`, `follows`, `project_id`,
`license`, `downloads`, `created_timestamp`, `modified_timestamp`, plus **`client_side` and
`server_side`, both marked *"deprecated — use `environment` instead"*** and both still working
live (`[["client_side:required"]]` → 324 hits for `query=sodium`). `[corrected 2026-09-11: 3 refuters,
https://docs.modrinth.com/openapi.yaml `paths./search.get.parameters[facets].description`]`
`created_timestamp` / `modified_timestamp` take **Unix timestamps**. `[docs]`

**`project_type:` is rewritten to `project_types` — the *plural, all-versions* field.**
`[live 2026-09-11]` So `project_type:mod` does **not** mean "primary type is mod": live,
`facets=[["project_type:mod"],["all_project_types:modpack"]]` returns 3 hits whose displayed
`project_type` is `modpack` (`exodus`, `performized`, `imaginefun`, each with
`all_project_types: ["mod","modpack"]`). It is rare — the top 100 by downloads under
`project_type:mod` are all `project_type == "mod"` — but a mod browser that trusts the facet
alone will occasionally list a modpack. Filter the hit's own `project_type` field as well.

### Search response

Top level: `hits`, `offset`, `limit`, `total_hits`. `[live]`

Hit object keys, **exactly as returned live on 2026-09-11** `[live]`
`GET https://api.modrinth.com/v2/search?query=sodium&limit=1`:

```
project_id, project_type, all_project_types, slug, author, author_id,
organization, organization_id, title, description, categories,
display_categories, versions, downloads, follows, icon_url,
date_created, date_modified, latest_version, license,
client_side, server_side, environment, disclosure_types,
gallery, featured_gallery, color
```

Notes:

* `versions` is the list of **game versions** the project supports (not version ids).
* `categories` includes loader names; `display_categories` is the curated subset shown
  in the Modrinth UI.
* `client_side` / `server_side` are `required` \| `optional` \| `unsupported` \|
  `unknown`. `[live]` (sodium: `client_side: "required"`, `server_side: "unsupported"`) —
  **but both are marked `deprecated: true` in the current spec (*"Deprecated — use
  `environment` instead"*), and so are the matching facet types.** `[corrected 2026-09-11: 3 refuters,
  https://docs.modrinth.com/openapi.yaml `components.schemas.ProjectResult`]` Read
  `environment` first and treat `client_side`/`server_side` as legacy fallbacks.
* **`environment` is a *required* property of `ProjectResult`** (as are `all_project_types`
  and `disclosure_types`) — not an undocumented extra. Its enum, verified against the spec
  `[live 2026-09-11]` `https://docs.modrinth.com/openapi.yaml` `EnvironmentEnum`:
  `client_and_server`, `client_only`, `client_only_server_optional`, `singleplayer_only`,
  `server_only`, `server_only_client_optional`, `dedicated_server_only`, `client_or_server`,
  `client_or_server_prefers_both`, `unknown`. On a *hit* it is an **array**; treat it as a set.
* `environment`, `all_project_types` and `disclosure_types` are **newer fields that the
  live v2 endpoint now returns** even though older third-party v2 write-ups do not list
  them. Treat every field as optional in the deserializer. `[live]`
* There is **no `date_modified` vs `updated` alias** — search hits use
  `date_created`/`date_modified`; the project object uses `published`/`updated`. `[live]`

## A.3 Project — `GET /v2/project/{id|slug}`

`[docs]` https://docs.modrinth.com/api/operations/getproject/

Top-level keys observed live for `sodium` `[live]`
`GET https://api.modrinth.com/v2/project/sodium`:

```
client_side, server_side, game_versions, environment, id, slug, project_type,
team, organization, title, description, body, body_url, published, updated,
approved, queued, status, requested_status, moderator_message, license,
downloads, followers, categories, additional_categories, loaders, versions,
icon_url, raw_icon_url, issues_url, source_url, wiki_url, discord_url,
donation_urls, gallery, color, thread_id, monetization_status
```

* `versions` here **is** an array of version ids (base62), unlike in a search hit.
* `loaders` on the project is a flat array, e.g. sodium → `["fabric","neoforge","quilt"]`. `[live]`
* `license` is an object `{id, name, url}`. `[docs]`
* `gallery[]` entries carry `url`, `featured`, `title`, `description`, `created`,
  `ordering`. `[docs]`
* `raw_icon_url` exists alongside `icon_url` (the un-resized original). `[live]`

**Docs/live conflict:** the docs page marks `client_side` / `server_side` as
*deprecated* in favour of `environment`. `[docs]`
https://docs.modrinth.com/api/operations/getproject/ — but the live v2 response still
returns all three. `[live]` Read `environment` when present, fall back to
`client_side`/`server_side`.

Batch: **`GET /v2/projects?ids=["AANobbMI","P7dR8mSH"]`** (URL-encoded JSON array)
returns a **plain JSON array** of project objects, verified live with 2 ids. `[live]`
The response is *not* wrapped and is *not* keyed by id — match results back by `id`
yourself, and do not assume request order is preserved.

## A.4 Versions — `GET /v2/project/{id|slug}/version`

`[docs]` https://docs.modrinth.com/api/operations/getprojectversions/

| Param | Format | Example |
| --- | --- | --- |
| `loaders` | JSON array of strings | `["fabric"]` |
| `game_versions` | JSON array of strings | `["1.20.1"]` |
| `featured` | boolean | `true` |
| `include_changelog` | boolean, default `true` | `false` |

`include_changelog=false` is explicitly recommended by the docs to cut response size
`[docs]` — **use it**; changelogs are markdown blobs and are not needed for a picker.

**Ordering (undocumented, verified empirically):** the array is returned
**newest-first by `date_published`**. `[live]`
`GET https://api.modrinth.com/v2/project/sodium/version?game_versions=["1.20.1"]`
returned 13 entries, strictly descending:

```
mc1.20.1-0.5.13-fabric        2025-03-03  release
mc1.20.1-0.5.12-beta.2-fabric 2025-02-26  beta
mc1.20.1-0.5.11               2024-06-29  release
... down to mc1.20-0.4.10     2023-06-07  release
```

**This example was wrong and is corrected here.** `[corrected 2026-09-11: 3 refuters, live
`GET https://api.modrinth.com/v2/project/sodium/version?game_versions=["1.20.1"]`]` In this
dataset the beta at index 1 is **older** (2025-02-26) than the release at index 0
(2025-03-03), so `array[0]` **is** the newest release and Sodium does *not* demonstrate the
trap. The general hazard is real — a beta can sit at `array[0]` — but the evidence for it is
**CurseForge's JEI**, not Modrinth's Sodium: the newest 1.20.1 Forge JEI file
(`jei-1.20.1-forge-15.59.0.210.jar`, `fileDate 2026-09-10T14:18:31.89Z`) has
`releaseType: 2` = **Beta** (B.4). `[live]` Use that one wherever the point needs proving.
A release-type preference still has to exist (D3); it just is not this row that proves it.

**`limit`, `offset` and `version_type` are real, first-class server parameters** on this
endpoint — merely absent from the OpenAPI spec. `[corrected 2026-09-11: labrinth
`routes/v2/versions.rs` `VersionListFilters { game_versions, loaders, featured, version_type,
limit, offset, include_changelog }` and `routes/v3/versions.rs` `version_list`, plus live
probes]` The server **sorts by `date_published` desc BEFORE** applying `.skip(offset).take(limit)`
— the source comment says so verbatim: *"Sort before applying limit/offset so that limit=N
returns the N newest versions"*. Live: `limit=2` → 2, `limit=1` → 1, `offset=1&limit=2` →
`[0.5.12-beta.2, 0.5.11]`, `version_type=beta` → 1 entry. The earlier "do not rely on it" was
overly cautious.

**One genuine trap:** `featured` is filtered **after** `limit`/`offset`, so
`featured=true&limit=1` live returned a **non-featured** version
(`mc26.2-0.9.2-beta.1-neoforge`, `featured: false`) through the auto-featured fallback.
**Never combine `featured` with `limit`.** `[corrected 2026-09-11: live probe + labrinth
`routes/v3/versions.rs`]`

### Version object

Keys observed live `[live]`:

```
game_versions, loaders, environment, id, project_id, author_id, featured,
name, version_number, changelog, changelog_url, date_published, downloads,
version_type, status, requested_status, files, dependencies
```

| Field | Type / values | Source |
| --- | --- | --- |
| `id` | base62 version id, e.g. `"OihdIimA"` | `[live]` |
| `version_number` | free text, e.g. `"mc1.20.1-0.5.13-fabric"` | `[live]` |
| `version_type` | `release` \| `beta` \| `alpha` | `[docs]` |
| `status` | `listed` \| `archived` \| `draft` \| `unlisted` \| `scheduled` \| `unknown` | `[docs]` |
| `game_versions` | `["1.20.1"]` | `[live]` |
| `loaders` | `["fabric","quilt"]` — **one version can serve several loaders** | `[live]` |
| `date_published` | ISO-8601 UTC, e.g. `"2025-03-03T17:45:49.132919Z"` | `[live]` |
| `featured` | bool | `[docs]` |

`dependencies[]`:

| Field | Type | Notes |
| --- | --- | --- |
| `version_id` | string \| null | pin to an exact version when set |
| `project_id` | string \| null | otherwise resolve latest matching |
| `file_name` | string \| null | used for embedded/bundled jars |
| `dependency_type` | `required` \| `optional` \| `incompatible` \| `embedded` | `[docs]` |

`files[]`:

| Field | Type | Notes |
| --- | --- | --- |
| `hashes.sha1` | hex string | **not individually required by the schema** — `VersionFileHashes` has no `required:` list; only the `hashes` *object* is required on `VersionFile` (`required: [hashes, url, filename, primary, size]`). In practice always present (labrinth writes `content_sha1` + `content_sha512` on every upload; 1454/1454 sampled live files had both). **Model as `Option<String>`.** `[corrected 2026-09-11: 3 refuters, https://docs.modrinth.com/openapi.yaml + `routes/v3/version_creation.rs`]` |
| `hashes.sha512` | hex string | same — schema-optional, present in practice. `Option<String>` |
| `url` | string | CDN URL, **already percent-encoded** |
| `filename` | string | usually **raw** (may contain `+` and spaces), **but not universally**: 293 `fabric-api` files from a 2021-01-23 legacy import carry `%2B` *literally in the `filename` field*, e.g. `"fabric-api-0.29.5%2B1.17.jar"`. Writing that verbatim produces exactly the `%2B`-named file this document warns about. `[corrected 2026-09-11: live probe over `GET /v2/project/fabric-api/version`]` See the trap below. |
| `primary` | bool | required |
| `size` | integer bytes | |
| `file_type` | full enum, verified against the spec `[live 2026-09-11]` (`FileTypeEnum`, https://docs.modrinth.com/openapi.yaml lines 406-416): `required-resource-pack`, `optional-resource-pack`, `sources-jar`, `dev-jar`, `javadoc-jar`, `unknown`, `signature`, **or `null`** | **`null` proves nothing.** `[corrected 2026-09-11: 3 refuters, live probes]` It is uploader-supplied and almost never set — across ~3,000 live versions of 13 popular projects **no non-null `file_type` was observed at all**, sources jars included. Live counterexample: sodium `vgceLbdH` (`mc1.20-0.4.10`) has `sodium-fabric-mc1.20-0.4.10+build.27.jar` (`primary: true`, `file_type: null`) **and** `sodium-fabric-mc1.20-0.4.10+build.27-sources.jar` (`primary: false`, `file_type: null`). Spec description: *"The type of the additional file, used mainly for adding resource packs to datapacks"*. Prism never reads it. |
| `id` | present live, not in the docs schema | `[live]` |

`[docs]` https://docs.modrinth.com/api/operations/getprojectversions/ + `[live]`.

### The "primary file" rule and the `+` trap

Live example `[live]`:

```
files[0].url      https://cdn.modrinth.com/data/AANobbMI/versions/OihdIimA/sodium-fabric-0.5.13%2Bmc1.20.1.jar
files[0].filename sodium-fabric-0.5.13+mc1.20.1.jar
files[0].primary  true
files[0].file_type null
```

* **`url` and `filename` differ.** `+` in the filename is `%2B` in the URL. **Always
  request `files[].url` verbatim** — never rebuild it from `filename`, and never
  re-encode it. Conversely, **write to disk using `filename`**, not the last URL path
  segment. Getting this backwards produces either a 404 or a file literally named
  `sodium-fabric-0.5.13%2Bmc1.20.1.jar`.
* **Legacy caveat: `filename` is not always raw.** `[corrected 2026-09-11: live probe over
  `fabric-api`]` 293 `fabric-api` files (all `date_published` 2021-01-23, a bulk legacy
  import) have `filename` containing a literal `%2B` — `"fabric-api-0.29.5%2B1.17.jar"` —
  with a matching `url` segment. Writing `filename` verbatim is still the rule, but for
  those rows the *provider's own* name is already percent-encoded. **Do not "fix" it by
  decoding**: decoding would break the hash-keyed index and the on-disk↔metadata match, and
  you cannot tell a legacy `%2B` from a filename that genuinely contains those three
  characters. Write what the API says; the D1 index records the same string.
  (Spaces are legal too: labrinth's `is_safe_file_name` rejects only `/`, `\` and
  non-normal path components.)
* Pick the file where `primary == true`. **`no primary` is not an exotic edge case — it is
  widespread legacy data.** `[corrected 2026-09-11: live probes]` `fabric-api` has **563 of 1196**
  versions with no primary file (all single-file, e.g. `YDlOLdpm` `0.67.0+1.18.2`,
  `primary: false`, 2022-11-20 and older); `iris` has 12 of 233. labrinth now forces
  `primary` when `total_files_len == 1`, so *new* uploads always have one, but roughly half
  of `fabric-api`'s history exercises the fallback. Prism copes only because its
  `isPreferred = primary || files.count() == 1` treats single-file versions as preferred.
* **Do not use `file_type` as the sources-jar filter** — it is null on sources jars too
  (see the table above). `[corrected 2026-09-11: 3 refuters]` The reliable rule is: `primary == true`;
  failing that, the first file whose name ends `.jar` and does **not** end
  `-sources.jar` / `-dev.jar` / `-javadoc.jar`.

**CDN URL shape: opaque — do not build one.** `[corrected 2026-09-11: live probe + labrinth
`routes/v3/version_creation.rs`]` Current labrinth writes
`data/{project_id}/versions/{version_id}/{urlencoding::encode(file_name)}`, but **legacy
versions use the URL-encoded `version_number` as the middle segment instead of the version
id — and sometimes a since-renamed one.** Live counts: 564 of 1196 `fabric-api` files
(`https://cdn.modrinth.com/data/P7dR8mSH/versions/0.60.0%2B1.19.2/fabric-api-0.60.0%2B1.19.2.jar`),
10 of 244 `sodium` files (`…/AANobbMI/versions/mc1.19-0.4.2/…`), and sodium `6YGRDUVT` uses
`mc1.18-0.40-alpha5`, which matches **neither** its id **nor** its current `version_number`
(`mc1.18-0.4.0-alpha5`). **Treat `files[].url` as an opaque string.**

## A.5 Batch and hash endpoints

| Endpoint | Method | Body / params | Source |
| --- | --- | --- | --- |
| `/v2/projects?ids=[...]` | GET | URL-encoded JSON array → plain array response | `[live]` |
| `/v2/versions?ids=[...]` | GET | same shape; verified live with `ids=["OihdIimA"]` → plain array | `[live]` |
| `/v2/version/{id}` | GET | single version by id → a bare version object. Used by dependency resolution when a `version_id` is pinned (C.5); **it was missing from this table until the completeness pass** `[corrected 2026-09-11: see E.1.2]` | `[docs]` https://docs.modrinth.com/api/operations/getversion/ |
| `/v2/version_file/{hash}` | GET | single hash → one version object; takes `?algorithm=sha1\|sha512` (**query param, required in the spec, default `sha1`**) and `?multiple=`. Unknown hash → **HTTP 404 with an empty body** `[live 2026-09-11]` | `[docs]` operationId **`versionFromHash`** → https://docs.modrinth.com/api/operations/versionfromhash/ `[corrected 2026-09-11: 2 refuters — the old citation pointed at `versionsfromhashes`, which is the *bulk POST* operation]` |
| `/v2/version_files` | **POST** | `{"hashes":[...], "algorithm":"sha1"\|"sha512"}` | `[docs]` operationId **`versionsFromHashes`** (plural — this is the bulk POST) → https://docs.modrinth.com/api/operations/versionsfromhashes/ `[corrected 2026-09-11: the row above no longer cites this page, so "same" would have pointed at the wrong operation]` |
| `/v2/version_files/update` | **POST** | see below. **labrinth marks this route DEPRECATED** in favour of the undocumented `POST /v2/version_files/update_many`, which takes the same body but returns `hash → [Version]` instead of `hash → Version` `[corrected 2026-09-11: 2 refuters, labrinth `routes/v2/version_file.rs`]` | `[docs]` https://docs.modrinth.com/api/operations/getlatestversionsfromhashes/ |
| ~~`/v2/version_files/latest`~~ | — | **THIS ENDPOINT DOES NOT EXIST.** `[corrected 2026-09-11: 3 refuters — live `GET` and `POST` both return HTTP 404 `{"error":"not_found","description":"the requested route does not exist"}`; the OpenAPI spec defines only `/version_file/{hash}`, `/version_file/{hash}/update`, `/version_files`, `/version_files/update`]` The bulk "latest" lookup **is** `POST /v2/version_files/update`. labrinth's v2 scope additionally registers the undocumented `/update_many` and `/update_individual` (body `{algorithm, hashes: [{hash, loaders, game_versions, version_types}]}` — per-hash filters). | `[source]` labrinth `routes/v2/version_file.rs` + live probe |

### `POST /v2/version_files` — identify installed jars

```json
{ "hashes": ["<sha1 or sha512 hex>", ...], "algorithm": "sha1" }
```

Response is a **map keyed by the hash string**, value = a full version object.
Hashes with no match are simply **absent from the map** (not null, not `{}`). `[docs]`
https://docs.modrinth.com/api/operations/versionsfromhashes/

Three behaviours that will silently look like "nothing is installed" if you get them wrong
`[corrected 2026-09-11: 2 refuters, live probes + labrinth `routes/v2/version_file.rs`]`:

* **Hex must be lowercase.** Matching is case-sensitive against the DB-stored lowercase hex.
  Submitting the same sha1 in UPPERCASE returns **HTTP 200 `{}`** on both `/version_files`
  and `/version_files/update`. Lowercase before sending, and key your local lookup on the
  lowercase string.
* **`algorithm` is not validated.** The enum lives only in the docs; the server accepts any
  string. `algorithm: "md5"` → 200 `{}` (no 400), and a sha1 hash sent with
  `algorithm: "sha512"` → 200 `{}`. **A wrong algorithm is indistinguishable from "no
  matches".** Always send the algorithm that matches the hash length — or omit it, since
  labrinth's `default_algorithm_from_hashes` infers sha512 for ≥128 hex chars, else sha1.
* **An empty `hashes` array returns 200 `{}` today** (Prism issue #3211 saw HTTP 400 for
  this in 2024 on Prism 9.1; Prism PR #4840 now skips the request when no hashes remain for
  a loader, and #4923 fixed the loader-index increment that #4840 broke). Skip the request
  rather than relying on the current tolerance.

### `POST /v2/version_files/update` — the update check

This is the endpoint Prism uses to check for Modrinth updates.

```json
{
  "hashes":        ["<hash>", ...],
  "algorithm":     "sha1",
  "loaders":       ["fabric"],
  "game_versions": ["1.20.1"],
  "version_types": ["release", "beta"]        // optional
}
```

* **Documented-required, server-optional: only `hashes` is actually enforced.**
  `[corrected 2026-09-11: 3 refuters, labrinth `routes/v2/version_file.rs` `ManyUpdateData` +
  live probes]` The OpenAPI schema does mark them required (`HashList` requires
  `hashes`+`algorithm`; `GetLatestVersionsFromHashesBody` requires `loaders`+`game_versions`),
  but labrinth declares `algorithm: Option<String>`, `loaders: Option<Vec<String>>`,
  `game_versions: Option<Vec<String>>`, `version_types: Option<Vec<VersionType>>` and the SQL
  uses `cardinality($n) = 0 OR …`, so an omitted filter **matches everything**. Live: omitting
  `game_versions` returned Sodium `mc26.2-0.9.2-beta.1-*` for a 1.20.1 hash — **omission
  silently widens the update check instead of erroring.** A 400 will not catch your mistake;
  always send `loaders` and `game_versions` yourself. (This is also what C.6 §3 describes for
  Prism, which omits them when absent — the two sections now agree.)
* `version_types` is optional, values `release` \| `alpha` \| `beta`. `[docs]`
* Response: map `hash → version object` (the **latest** version matching the given
  loader + game version constraints). `[docs]`
  https://docs.modrinth.com/api/operations/getlatestversionsfromhashes/
* **An update exists iff the returned version's *file hash* differs from the hash you
  submitted — NOT iff the ids differ.** `[corrected 2026-09-11: live + labrinth's own source
  comment above `update_files_route`]` One file hash can be attached to several versions
  (V1 and V2 shipping the same jar). An id-based check then reports an update **forever**:
  labrinth's comment spells out the failure — *"user updates to V2, but it's the same file,
  so it's the same hash H, and the update button stays"*. That is precisely why
  `/version_files/update` is marked DEPRECATED in favour of `update_many` and documented as
  *"will only take the first version for each file hash"*. Prism gets this right
  (`projectVer.hash != hash`, C.6 §6). Use `update_many` and check whether your local hash
  appears among the returned versions' files, or compare the returned primary file's hash to
  the local one. The endpoint returns the current version when it is already latest, not an
  empty result.

Practical consequence: one POST checks the whole mods folder. Batch it; do not loop.

## A.6 Icons — format is NOT stable

Two live projects, two different extensions `[live]`:

```
sodium      icon_url = https://cdn.modrinth.com/data/AANobbMI/295862f4724dc3f78df3447ad6072b2dcd3ef0c9_96.webp
fabric-api  icon_url = https://cdn.modrinth.com/data/P7dR8mSH/icon.png
```

* Modern uploads are **`.webp`, resized, with a `_{size}` suffix** (`_96` here).
* Older projects still serve **`icon.png`** at the original path.
* Gallery images are also `.webp` with a `_{size}` suffix `[live]`
  (`.../images/d84313e6..._350.webp`).
* `icon_url` may be **`null`** for projects with no icon. `[docs]`

**Never infer the format from the extension alone — sniff the magic bytes.** See
section D for why this collides with Slint's compiled-in `image` features.

## A.7 Loader names (authoritative list)

`GET https://api.modrinth.com/v2/tag/loader` `[live]` returns each loader's `name`,
`icon` and `supported_project_types`. Loaders whose `supported_project_types` contains
`mod`:

```
babric, bta-babric, bukkit, bungeecord, datapack, fabric, folia, forge,
geyser, java-agent, legacy-fabric, liteloader, modloader, neoforge,
nilloader, ornithe, paper, purpur, quilt, rift, spigot, sponge,
velocity, waterfall
```

The four that matter for us: **`fabric`, `forge`, `neoforge`, `quilt`** — all lowercase,
all distinct. `[live]`

### Forge vs NeoForge for 1.20.1

**Modrinth treats `forge` and `neoforge` as completely separate loader tags.** `[live]`
There is no aliasing on the API side. For 1.20.1 — the last version where both loaders
exist and are broadly ABI-compatible — this means:

* A search facetted on `["categories:neoforge"]` **will not** return Forge-only mods
  that happen to run on NeoForge 1.20.1.
* Many 1.20.1 mods tag **only** `forge` yet load fine under NeoForge 1.20.1.
* Conversely a mod may tag both, and a single Modrinth *version* may list
  `loaders: ["forge","neoforge"]`.

Any compatibility widening (searching `forge` OR `neoforge` when the instance is
NeoForge 1.20.1) is a **client-side policy decision**, not something the API does.
Whether Prism does this is answered in section C.

## A.8 Game versions — `GET /v2/tag/game_version`

Feeds the MC-version filter dropdown. `[live]`

```json
{"version":"26.3-rc-1","version_type":"snapshot","date":"2026-09-10T11:28:25Z","major":false}
```

| Field | Notes |
| --- | --- |
| `version` | the string to put in the `versions:` facet / `game_versions` array |
| `version_type` | `release` \| `snapshot` \| `beta` \| `alpha` |
| `date` | ISO-8601 |
| `major` | bool — true for headline releases (`1.20`, `1.21`), useful for grouping |

Filter to `version_type == "release"` for the default dropdown; the full list is
several hundred entries and is dominated by snapshots. Cache it — it changes weekly at
most.

Sibling tag endpoints: `/v2/tag/loader` (A.7), `/v2/tag/category`,
`/v2/tag/license`. `[docs]` https://docs.modrinth.com/api/

---

# B. CurseForge Core API v1

Base URL `https://api.curseforge.com/v1`. `[docs]` https://docs.curseforge.com/rest-api/
`[repo]` `crates/nebula-core/src/curseforge.rs:19`

**Everything in this section marked `[live]` was executed against the real API on
2026-09-11** using the key already committed in this repo, so field names and enum
values below are observed, not transcribed.

## B.1 Auth and rate limits

* Header: **`x-api-key: <key>`** (case-insensitive; this repo sends `X-API-KEY`
  `[repo]` `crates/nebula-core/src/curseforge.rs:210`).
* **Without a key every request returns HTTP 403.** `[live]`
* **`HEAD` is unreliable — the behaviour is cache-dependent, not a flat 404.**
  `[corrected 2026-09-11: 2 refuters, live `curl -I`]` `HEAD /v1/mods/238222` returned **200**
  (`X-Cache: Hit from cloudfront`, `Age: 99`) because a `GET` had already populated the
  CloudFront cache; `HEAD` on an **uncached** mod (`/v1/mods/394468`) returned **404**; and
  `HEAD /v1/mods/search` returns **405 Method Not Allowed**. The guidance is unchanged —
  **use GET everywhere** — but the mechanism is cache state, not a missing method.
* **No rate-limit headers are returned at all.** A successful `GET /v1/mods/238222`
  response carries `Content-Type`, `Transfer-Encoding`, `Connection`, `Date`, `Age` and
  CloudFront headers (`X-Cache`, `Via`, `X-Amz-Cf-Pop`, `X-Amz-Cf-Id`); a
  `GET /v1/mods/search` response carries `Content-Length` and `Server: Kestrel` **instead of**
  `Transfer-Encoding`. `[corrected 2026-09-11: 2 refuters — the original list omitted `Date`, `Age`,
  `Server` and `Content-Length`]` The substantive claim holds and was re-verified: **no
  `X-Ratelimit-*`, no `Retry-After`, on either endpoint**, and 30 back-to-back search requests
  all returned 200 with no 429. The rendered docs mention rate limits nowhere at all. `[live]`

  **Consequence:** unlike Modrinth (A.1), adaptive backoff is impossible — you cannot
  see how much budget is left. Self-throttle, and treat 429 as the only signal.

### Getting a key, and whether it may be embedded

Keys are **not self-serve.** *"API access will be granted to each requester as a unique
key which can be generated after 3rd party developers submit an application… reviewed by
the Overwolf team."* Stated review criteria include *"How it takes Authors' consent when
it comes to third party distribution of their creations."* `[docs]`
https://support.curseforge.com/support/solutions/articles/9000208346-about-the-curseforge-api-and-how-to-apply-for-a-key

**The governing document is NOT the ToS linked from docs.curseforge.com.** That one is
the *CurseForge for Studios Terms of Use*, an agreement with a game studio; it never
mentions API keys. The document that binds third-party Core API consumers is the
**CurseForge 3rd Party API Terms and Conditions** `[docs]`
https://support.curseforge.com/support/solutions/articles/9000207405-curse-forge-3rd-party-api-terms-and-conditions

Controlling clause, verbatim (§ *Use of Platform API*):

> "each Developer will be issued a unique API Key (the "API Key"), which is
> **non-transferable and may not be shared with any third party**. Developer may not
> disclose the API Key to any third party, except to such employees who are subject to
> corresponding confidentiality obligations."

**Read plainly, publishing a key in a public repository is disclosure to third parties
and is not permitted. There is no open-source exception in the text.**

Two more clauses from the same document that directly constrain a launcher
(§ *Restrictions and Obligations of Developer*), verbatim:

> "Developer shall not … (e) conceal your identity or geographic location when accessing
> the API, including accessing the API through a proxy server or VPN; or (e) **save or
> cache any data obtained through the API or SDK**."

(The duplicated `(e)` is CurseForge's own typo.) **A literal reading forbids caching
search results, mod metadata and icons** — which every launcher including Prism does.
This is a real, unresolved tension, not a footnote. See Risks.

There is **no published numeric rate limit**, no `x-ratelimit-*` headers (verified live,
B.1), and no `429` declared in the swagger. The only volume language is contractual:

> "Upon the External App exceeding **a certain quota, to be decided and updated from time
> to time upon Overwolf's sole discretion**, continued use … may … require Developer to
> enter into a written licensing agreement."

`[docs]` same URL.

### What Prism does

* **Ships a key in the public repo, in plaintext**: `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/CMakeLists.txt line 278 —
  `set(Launcher_CURSEFORGE_API_KEY "$2a$10$wuAJuNZuted3NORVmpgUC.m8sI.pv1tOPKZyBgLFGjxFp/br0lZCC" CACHE STRING ...)`,
  substituted into `BuildConfig.FLAME_API_KEY`.
* **Offers a user override**: setting `FlameKeyOverride` / `CFKeyOverride`; if both the
  override and the built-in key are empty, the `SupportsFlame` capability bit is never
  set and **every CurseForge UI surface is hidden**. `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/Application.cpp
* **Passes the ToS liability downstream** rather than claiming the embedding is allowed.
  README, verbatim `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/README.md :
  > "Go through CMakeLists.txt and change Prism Launcher's API keys to your own or set
  > them to empty strings (`""`) to disable them… Note that if you build this software
  > without removing the provided API keys in CMakeLists.txt you are accepting the
  > following terms and conditions: [CurseForge 3rd Party API Terms and Conditions]"
* Attaches `x-api-key` to **both** the API host and `edge.forgecdn.net`. `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/net/ApiHeaderProxy.h

This repo already ships a key as a compile-time constant inherited from upstream Nebula
`[repo]` `crates/nebula-core/src/curseforge.rs:20`. See "Decisions" for the policy call.

### CDN key enforcement is coming (or is late)

CurseForge announced that direct CDN downloads would require an API key, *"optional now,
**required mid-July**"* — the blog gives the exact date: *"On **July 16th**, a valid key will
be required, and any request without one will be rejected"* `[corrected 2026-09-11: live re-read of
the blog post]` — returning `401 {"error":"Unauthorized","message":"A valid
api-key is required."}`, accepted as header `x-api-key` (recommended) or query param
`?api-key=`. `[docs]`
https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/

**Observed contradiction:** on 2026-09-11 — about two months past that date — an
unauthenticated request to `edge.forgecdn.net` still returned 302 → 200 with real bytes.
`[live]` Enforcement is evidently not active on that path yet. **Handle 401 anyway.**

Note also that Prism's header proxy matches only the host `edge.forgecdn.net`, while the
302 redirects to `mediafilez.forgecdn.net` — whether the header survives the redirect is
UNVERIFIED, and is the obvious place a 401 will first appear. `[prism]`

## B.2 Search — `GET /v1/mods/search`

Verified live: `gameId=432&classId=6&searchFilter=jei&gameVersion=1.20.1&modLoaderType=1&sortField=2&sortOrder=desc&pageSize=1&index=0` → HTTP 200. `[live]`

| Param | Value / notes |
| --- | --- |
| `gameId` | **432** = Minecraft (required) `[live]` |
| `classId` | **6** = Mc-Mods, and it is **effectively mandatory for a mod browser**: without it a `gameId=432` search is dominated by modpacks (live `searchFilter=optimization&pageSize=10` → 9 of 10 hits are `classId 4471`). `[live 2026-09-11]` Authoritative list from `GET /v1/categories?gameId=432&classesOnly=true` `[live 2026-09-11]`: **5** bukkit-plugins, **6** mc-mods, **12** texture-packs (Resource Packs), **17** worlds, **4471** modpacks, **4546** customization, **4559** mc-addons, **6552** shaders, **6945** data-packs. Prism's identical table lives in **`FlameAPI.cpp` (`g_classIDMappings`, ~line 174)**, not `FlameAPI.h` — the header only declares `getClassId`/`getResourceType` `[corrected 2026-09-11: 2 refuters]` |
| `categoryId` / `categoryIds` | **Not an override — an intersection, and the list itself is AND, not OR.** `[corrected 2026-09-11: live probes]` JEI (238222) is in 421 (library-api) and 423 (map-information), not 424 (cosmetic): `categoryId=421&categoryIds=[423]` → 1; `categoryId=421&categoryIds=[424]` → 0; `categoryIds=[421,424]` → 0; `categoryIds=[421]` → 1. **A multi-category UI filter therefore narrows, not widens.** Documented **max 10 ids** `[docs]` is **not enforced** (`categoryIds=[1..11]` → 200). |
| `gameVersion` / `gameVersions` | **Not an override — a union.** `[corrected 2026-09-11: live probes]` With `slug=fabric-api&classId=6`: `gameVersion=1.20.1&gameVersions=["1.1"]` → 1; `gameVersion=1.1&gameVersions=["1.20.1"]` → 1; `gameVersion=1.1` alone → 0. The scalar is never discarded. The *list* is OR (`["1.1","1.2","1.3","1.20.1"]` → 1). Documented **max 4** `[docs]` is **not enforced** (5 values → 200 and the 5th still matched). (The swagger's `gameVersions` text even says *"maximum allowed category ids per query is 4"* — a copy-paste slip.) |
| `searchFilter` | free text — "in the mod name and author" `[docs]` |
| `sortField` | int enum, below |
| `sortOrder` | `asc` \| `desc` |
| `modLoaderType` / `modLoaderTypes` | **Union, not override** `[corrected 2026-09-11: live probes]`: `slug=fabric-api&gameVersion=1.20.1&modLoaderType=1&modLoaderTypes=[4]` → 1 **and** `modLoaderType=4&modLoaderTypes=[1]` → 1, while `modLoaderType=1` alone → 0. Documented **max 5** is **not enforced** (`[1,2,3,4,5,6]` → 200). Encoding traps: `modLoaderTypes=[]` → unfiltered; **`modLoaderTypes=1` without brackets → 200 with 0 results, silently**; `modLoaderTypes=[Bogus]` → 200, silently unfiltered. Loader **names** are accepted and filter correctly (`modLoaderType=Fabric`, `modLoaderTypes=[Fabric]`), matching the swagger's own example; Prism sends ints. The documented *"must be coupled with gameVersion"* `[docs]` is **not validated** — `searchFilter=jei&modLoaderType=1` alone → 200, totalCount 262. Prism never sends the singular form on search at all (plural only, since PR #1560, 2023-08-23); singular is used only on `/mods/{id}/files`, and only when exactly one loader bit is set. |
| `gameVersionTypeId` | int |
| `slug` | **`slug` + `classId` yields a unique result** `[docs]` |
| `authorId` / `primaryAuthorId` | `authorId` = *member of*; `primaryAuthorId` = *owner of* `[docs]` |
| `index` | offset, **`index + pageSize` must be ≤ 10000** |
| `pageSize` | **1–50** |

### Hard limits — both verified live

```
pageSize=100  → HTTP 400
  {"errors":{"PageSize":["The field PageSize must be between 1 and 50."]},
   "type":"https://tools.ietf.org/html/rfc9110#section-15.5.1",
   "title":"One or more validation errors occurred.","status":400,"traceId":"..."}

index=10000&pageSize=50 → HTTP 400
  {"message":"Index + PageSize cannot exceed 10000"}
```
`[live]`

Note the two errors have **completely different body shapes** — one is an ASP.NET
validation-problem document, the other a bare `{"message": ...}`. Parse defensively.

**`pagination.totalCount` is capped at 10000 and is not the true total.** `[live 2026-09-11]`
`GET /v1/mods/search?gameId=432&classId=6&pageSize=1` → `{"index":0,"pageSize":1,
"resultCount":1,"totalCount":10000}` for a corpus far larger than that. Narrow queries report
honest counts (JEI: 151). Never render `totalCount` as "N results" without clamping language,
and never page past `index + pageSize = 10000` (hard 400, above).

### `sortField` enum `[docs]` `https://api.curseforge.com/swagger/v1/swagger.json` (`ModsSearchSortField`)

`[corrected 2026-09-11: 2 refuters — the table was attributed to Prism, but Prism's `getSortingMethods()` defines only rows 1-8; rows 9-13 come from the served swagger (1-13) and the rendered docs (1-12). All 13 return HTTP 200 live; `sortField=0` and `=14` return 400 `"The value 'N' is invalid."`]`

| Value | Name |
| --- | --- |
| 1 | Featured |
| 2 | Popularity |
| 3 | LastUpdated |
| 4 | Name |
| 5 | Author |
| 6 | TotalDownloads |
| 7 | Category |
| 8 | GameVersion |
| 9 | EarlyAccess |
| 10 | FeaturedReleased |
| 11 | ReleasedDate |
| 12 | Rating |
| 13 | **Relevancy** |

**Three-way disagreement.** The live OpenAPI document served by the API itself
(`https://api.curseforge.com/swagger/v1/swagger.json`, `ModsSearchSortField`) declares
**13** values including `Relevancy`. The rendered docs at
https://docs.curseforge.com/rest-api/ stop at **12**. PrismLauncher exposes only
**1–8** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameAPI.cpp.
Prism also pins `sortOrder=desc` unconditionally — its UI exposes the field but not the
direction. `[prism]`

### `modLoaderType` enum `[prism]` `FlameAPI::getMappedModLoader`

| Value | Loader |
| --- | --- |
| 0 | Swagger and the rendered docs name it **`Any`** — **but on the wire it is not a wildcard: it filters literally and returns nothing.** `[corrected 2026-09-11: 2 refuters, live probes]` `gameId=432&classId=6&searchFilter=sodium&gameVersion=1.20.1&modLoaderType=0` → HTTP 200, `totalCount: 0` (vs 32 for `=1`, 35 for `=4`); same for `modLoaderTypes=[0]`. **To mean "any loader", omit the parameter.** Prism never sends 0 — it drops the filter when `getMappedModLoader` returns 0. |
| 1 | **Forge** |
| 2 | Cauldron |
| 3 | LiteLoader |
| 4 | **Fabric** |
| 5 | **Quilt** |
| 6 | **NeoForge** |

### Search response — live key list

`GET /v1/mods/search?...searchFilter=jei...` `[live]`:

Top level: `data`, `pagination`.
`pagination` = `{"index":0,"pageSize":1,"resultCount":1,"totalCount":151}`.

`data[]` object keys, exactly as returned:

```
screenshots, id, gameId, name, slug, links, summary, status, downloadCount,
isFeatured, primaryCategoryId, categories, classId, authors, logo, mainFileId,
latestFiles, latestFilesIndexes, latestEarlyAccessFilesIndexes, dateCreated,
dateModified, dateReleased, allowModDistribution, gamePopularityRank,
isAvailable, hasCommentsEnabled, thumbsUpCount, featuredProjectTag
```

Live values for JEI (id 238222, slug `jei`) `[live]`:

```jsonc
"links":  {"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei",
           "wikiUrl":"", "issuesUrl":"...", "sourceUrl":"..."}
           // CORRECTED 2026-09-11 (live probe): absent links are frequently **null**,
           // and one object can mix both forms. JEI just happens to use "". Live
           // GET /v1/mods/search?gameId=432&classId=6 returned id 1546263
           // arizona-music-disc {"wikiUrl":null,"issuesUrl":null,"sourceUrl":null},
           // id 1475622 newtoolscraft (all three null), and id 1683128 end-update-je
           // {"wikiUrl":"","issuesUrl":"","sourceUrl":null}. The swagger declares them
           // plain `string` with no nullable flag, so the SPEC is wrong too.
           // => Option<String>, and treat null / "" / absent identically.
           // (Prism tolerates both via linksObj["wikiUrl"].toString().)

"logo":   {"id":29069, "modId":238222, "title":"635838945588716414.jpeg",
           "description":"",
           "thumbnailUrl":"https://media.forgecdn.net/avatars/thumbnails/29/69/256/256/635838945588716414.jpeg",
           "url":"https://media.forgecdn.net/avatars/29/69/635838945588716414.jpeg"}
           // CurseForge icons are JPEG/PNG on media.forgecdn.net — NOT webp.
           // thumbnailUrl is 256x256; use it, not url.

"authors":[{"id":17072262,"name":"mezz","url":"https://www.curseforge.com/members/mezz",
            "avatarUrl":"...","userTierInfo":{"tierLevel":6,"downloadsThreshold":500000000}}]

"latestFilesIndexes[0]":
          {"gameVersion":"26.2","fileId":8851049,
           "filename":"jei-26.2-neoforge-30.32.0.212.jar",
           "releaseType":2,"gameVersionTypeId":86297,"modLoader":6}

"allowModDistribution": true
```

`latestFilesIndexes` is the cheap way to render "available for MC x with loader y"
without a second request — it carries `gameVersion`, `fileId`, `filename`,
`releaseType` and `modLoader` (the same int enum as `modLoaderType`). `[live]`

**`modLoader` is nullable — parse it as `Option<u8>`.** `[corrected 2026-09-11: live probes +
swagger `components.schemas.FileIndex.modLoader` = `{nullable: true, $ref: ModLoaderType}`]`
Live search results for JEI (238222), geckolib (388172), cloth-config (348521), jade (324717),
charm-of-undying (316873) and owo-lib (532610) each contain `latestFilesIndexes` entries with
`"modLoader": null`. See B.4 for how to recover the loader from the file object when it is.

## B.3 Mod and file endpoints

| Endpoint | Method | Notes |
| --- | --- | --- |
| `/v1/mods/{modId}` | GET | single mod |
| `/v1/mods` | POST | batch, body `{"modIds":[...]}` `[prism]` FlameAPI.h |
| `/v1/mods/{modId}/files` | GET | **nine** query params, not five `[corrected 2026-09-11: 2 refuters, swagger `paths['/v1/mods/{modId}/files'].get.parameters`]`: `gameVersion`, `modLoaderType`, `gameVersionTypeId`, `index`, `pageSize`, **`olderThanProjectFileId`** (int32, nullable), **`releaseTypes`** (array of `ProjectFileReleaseType`, `style=form`/`explode`), **`platformType`** (`PlatformType`, nullable), **`clientCompatible`** (bool — *"filter OUT any file that isn't client compatible"*). `index` is bounded the same way as search: `index + pageSize <= 10000` |
| `/v1/mods/{modId}/files/{fileId}/download-info` | GET | query `updateFrom`; response `ApiResponseOfListOfDownloadInfo` — **undocumented in this facts doc until now** `[corrected 2026-09-11: swagger]` |
| `/v1/mods/{modId}/files/{newFileId}/deltas/{oldFileId}` | GET | binary delta between two files `[corrected 2026-09-11: swagger]` |
| `/v1/mods/files` | POST | batch, body `{"fileIds":[...]}` `[prism]` |
| `/v1/mods/{modId}/files/{fileId}` | GET | single file |
| `/v1/mods/{modId}/files/{fileId}/download-url` | GET | see B.5 |
| `/v1/mods/{modId}/files/{fileId}/changelog` | GET | HTML changelog `[prism]` |
| `/v1/mods/{modId}/description` | GET | HTML body `[prism]` |
| `/v1/categories?gameId=432&classId={id}` | GET | category tree `[prism]` |
| `/v1/fingerprints` | POST | see B.6 |

**`POST /v1/mods` body is not just `{"modIds":[...]}`.** The declared schema is
`GetExpandedModsByIdsListRequestBody` = `{"modIds":[int], "devModIds":[int],
"filterPcOnly": bool|null}`. `{"modIds":[...]}` is a valid subset. `[docs]`
`https://api.curseforge.com/swagger/v1/swagger.json`. `POST /v1/mods/get-mods-by-ids` is
an exact alias.

**`pageSize` on `/mods/{modId}/files`: documented max 50, but the server *honours*
oversized values — it neither clamps nor rejects.** `[corrected 2026-09-11: 2 refuters, live probes]`
`GET /v1/mods/238222/files?pageSize=10000` → HTTP 200 with **3599 file objects in one
response** and `pagination: {index:0, pageSize:10000, resultCount:3599, totalCount:3599}`;
`pageSize=51` → 51; `pageSize=100` → 100; omitting it → 50 (the *default*, which is what the
swagger text actually pins down). This is unlike `/v1/mods/search`, where `pageSize=100`
returns a hard **400**.

That is why Prism can request `pageSize=10000` `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameAPI.h
— it picks exactly 10000 because the one limit that *is* enforced is `index + pageSize <=
10000`. **The earlier explanation ("the server evidently clamps") was wrong; the advice is
unchanged — paginate properly** rather than pulling 3599 objects to find one file.

## B.4 File object — live key list

`GET /v1/mods/238222/files?gameVersion=1.20.1&modLoaderType=1&pageSize=2` `[live]`:

```
id, gameId, modId, isAvailable, displayName, fileName, releaseType, fileStatus,
hashes, fileDate, fileLength, downloadCount, fileSizeOnDisk, downloadUrl,
gameVersions, sortableGameVersions, dependencies, alternateFileId,
isServerPack, fileFingerprint, modules
```

Live values `[live]`:

```jsonc
"id":              8851009,
"displayName":     "15.59.0.210 for Forge 1.20.1",
"fileName":        "jei-1.20.1-forge-15.59.0.210.jar",
"releaseType":     2,          // 1 Release, 2 Beta, 3 Alpha
"fileStatus":      4,          // 4 = Approved
"fileDate":        "2026-09-10T14:18:31.89Z",
"fileLength":      1825651,    // download size in bytes
"fileSizeOnDisk":  4350698,    // UNCOMPRESSED size — NOT the download size
"downloadUrl":     "https://edge.forgecdn.net/files/8851/9/jei-1.20.1-forge-15.59.0.210.jar",
"fileFingerprint": 2298786945, // unsigned 32-bit, see B.6
"alternateFileId": 0,
"isServerPack":    false,
"hashes": [{"value":"3bbb49d85ee2f6a236f279816a62f0501d032db3","algo":1},
           {"value":"bdfe2c22f2ebb0b63536b521b5648492","algo":2}],
"gameVersions": ["Client","1.20.1","Forge","Server"],
"sortableGameVersions[0]": {"gameVersionName":"Client","gameVersionPadded":"0",
                            "gameVersion":"","gameVersionReleaseDate":"...",
                            "gameVersionTypeId":75208},
"dependencies": [],
"modules[0]": {"name":"META-INF","fingerprint":2071765406}
```

**Traps confirmed live:**

* `hashes[].algo`: **1 = SHA-1, 2 = MD5.** `[live]` (the 40-hex value is algo 1, the
  32-hex is algo 2). There is **no sha512** — Modrinth's preferred algorithm is
  unavailable here.
* **`gameVersions[]` is not a list of Minecraft versions.** It mixes MC versions with
  loader names and side names: `["Client","1.20.1","Forge","Server"]`. `[live]`
  Filtering on it naively will match `"Forge"` as if it were a game version. Use
  `sortableGameVersions[]` and its `gameVersionTypeId`, or filter server-side with the
  `gameVersion` / `modLoaderType` query params.
* `fileSizeOnDisk` (4.35 MB) ≠ `fileLength` (1.83 MB) — verify downloads against
  `fileLength`. `[live]`
* `releaseType: 2` (Beta) on the newest 1.20.1 Forge JEI shows that "newest file" and
  "newest release" genuinely diverge in practice. `[live]` **This — not the Modrinth Sodium
  example (A.4) — is the verified evidence for that divergence.**

### Recovering the loader from a file when `modLoader` is null

`FileIndex.modLoader` is nullable (B.2) and `File` has no loader field at all, so the loader
has to come out of `sortableGameVersions[]`. **The discriminator is `gameVersionTypeId`, and
the mapping is served by the API** `[live 2026-09-11]`
`GET /v1/games/432/version-types` (39 entries):

| `gameVersionTypeId` | `name` | Meaning |
| --- | --- | --- |
| **68441** | **Modloader** | `gameVersionName` is a **loader** (`Forge`, `Fabric`, `NeoForge`, `Quilt`); `gameVersion` is `""` |
| **75208** | **Environment** | `gameVersionName` is a **side** (`Client`, `Server`); `gameVersion` is `""` |
| 75125, 75208-adjacent numeric ids | `Minecraft 1.20`, … | `gameVersionName` is a real MC version and `gameVersion` is non-empty |
| 76871 | Shader Loader | shaderpack loaders |

Live for `jei-1.20.1-forge-15.59.0.210.jar` `[live 2026-09-11]`:

```jsonc
"gameVersions": ["Client", "1.20.1", "Forge", "Server"]
"sortableGameVersions": [
  {"gameVersionName":"Client",  "gameVersionTypeId":75208, "gameVersion":""},
  {"gameVersionName":"1.20.1",  "gameVersionTypeId":75125, "gameVersion":"1.20.1"},
  {"gameVersionName":"Forge",   "gameVersionTypeId":68441, "gameVersion":""},
  {"gameVersionName":"Server",  "gameVersionTypeId":75208, "gameVersion":""}
]
```

**Rule:** the file's loaders are
`sortableGameVersions.filter(gameVersionTypeId == 68441).map(gameVersionName)`, matched
case-insensitively against the `modLoaderType` names; its MC versions are the entries with a
non-empty `gameVersion`. This is strictly better than string-matching `gameVersions[]`, which
mixes all three kinds in one flat array. Cache the `version-types` list — it changes about as
often as a new loader appears. **A file with no 68441 entry has no declared loader** (Prism's
`ModLoaderType(0)` sentinel bucket, C.6) — do not treat that as "unsupported"; treat it as
"unknown, accept for any loader", which is what Prism does.

### Schema notes from the served OpenAPI document

`https://api.curseforge.com/swagger/v1/swagger.json` (NSwag-generated, `"title":
"CurseForge Core API"`) is a **superset** of the rendered docs. `[docs]`

* **Integer widths:** `fileLength`, `downloadCount`, `fileSizeOnDisk`, `fileFingerprint`
  and `pagination.totalCount` are **int64**; the rest are int32. *"Unless stated
  otherwise all int32 responses are unsigned."* `[docs]`
  https://docs.curseforge.com/rest-api/ — use `u64`/`u32` in Rust, not `i32`.
* **`File` fields the live response above did not include but the schema declares:**
  `exposeAsAlternative`, `parentProjectFileId`, `serverPackFileId`,
  `isEarlyAccessContent`, `earlyAccessEndDate`, `cookingInfo`. **Five of the six carry
  `"nullable": true`; `cookingInfo` does NOT** — it is a bare `{"$ref": "CookerInfo"}`
  (`CookerInfo = {cookerVersion: string, cookerRevision: string}`) with no nullable flag, and
  it does not appear in the rendered `File` schema on docs.curseforge.com at all (that table
  ends at `modules`). `[corrected 2026-09-11: 2 refuters, swagger
  `components.schemas.File.properties.cookingInfo`]` Make every field `Option<_>` anyway —
  the API omits nulls inconsistently, and here the schema is simply wrong.
* **`fileStatus` enum (15 values):** 1 Processing, 2 ChangesRequired, 3 UnderReview,
  **4 Approved**, 5 Rejected, 6 MalwareDetected, 7 Deleted, 8 Archived, 9 Testing,
  10 Released, 11 ReadyForReview, 12 Deprecated, 13 Baking, 14 AwaitingPublishing,
  15 FailedPublishing. `[docs]`
* **`downloadUrl` is NOT marked nullable in the schema** — yet it is literally `null` on
  the wire for blocked files `[live]` (B.5). Deserialize it as `Option<String>` and treat
  `null`, `""` and *absent* identically; Prism normalises all three to `""`. `[prism]`
* `ModAuthor` also has `avatarUrl`; `ModAsset` also has `modId`; `FileIndex`'s filename
  key is **`filename`** (lowercase n) while `File`'s is **`fileName`** (capital N).
  `[docs]` + `[live]`

`dependencies[]` = `{modId, relationType}`. The enum itself is the swagger's
`FileRelationType` `[docs]`; the integer→enum switch is in **`FlameModIndex.cpp`
(`FlameMod::loadIndexedPackVersion`, ~lines 186-212)**, not in `GetModDependenciesTask.cpp`,
which only holds the `if (verDep.type != DependencyType::REQUIRED) continue;` filter at line
148 that the "Prism's behaviour" column describes `[corrected 2026-09-11: 2 refuters]` `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameModIndex.cpp:

| Value | Meaning | Prism's behaviour |
| --- | --- | --- |
| 1 | EmbeddedLibrary | skipped |
| 2 | OptionalDependency | skipped |
| 3 | **RequiredDependency** | **auto-added** |
| 4 | Tool | skipped |
| 5 | Incompatible | skipped |
| 6 | Include | skipped |

## B.5 `downloadUrl == null` — the blocked-mod problem

**Quantified live:** of the **top 250** 1.20.1 mods by total downloads,
**5 (2%) have `allowModDistribution: false`** `[live]` — and they are not obscure:

```
448233 entityculling
433760 not-enough-animations
521480 skin-layers-3d
243707 corail-tombstone
636540 structory
```

For such a mod, e.g. `GET /v1/mods/448233/files?gameVersion=1.20.1&pageSize=1` `[live]`:

```jsonc
"id": 8287086,
"fileName": "entityculling-forge-1.10.5-mc1.20.1.jar",
"downloadUrl": null,          // <-- null, not ""
"isAvailable": true,          // still true!
"fileStatus": 4,              // still Approved
"fileFingerprint": 4018169999
```

* **`isAvailable` and `fileStatus` do NOT tell you it is blocked.** Only
  `downloadUrl == null` (or the project's `allowModDistribution == false`) does. `[live]`
* **`GET /v1/mods/448233/files/8287086/download-url` returns HTTP 403 with an empty
  body.** `[live]` It is not a fallback for blocked files.

### The CDN URL can be reconstructed — and it works

The `downloadUrl` for an unblocked file decomposes exactly as

```
https://edge.forgecdn.net/files/{fileId / 1000}/{fileId % 1000}/{fileName}
```

— **integer division and modulo, with no zero-padding** on the second segment `[live]`:

```
8851009 -> /files/8851/9/...     (009 -> 9, not "009")
8287086 -> /files/8287/86/...
3509043 -> /files/3509/43/...    [prism] tests/testdata/Packwiz/screenshot-to-clipboard-fabric.pw.toml
```

Nuance: **the CDN also accepts the zero-padded form** — `HEAD
https://edge.forgecdn.net/files/8851/**009**/jei-1.20.1-forge-15.59.0.210.jar` → 302 to
`mediafilez.forgecdn.net/files/8851/9/…`. `[corrected 2026-09-11: live probe]` The unpadded form is
what the API returns and what you would generate; both resolve. (Not that you should be
generating either — see below.)

**"First 4 digits / remaining digits" is wrong** as a general rule — it only coincides
with div/mod-1000 for 7-digit ids. An 8-digit id would split as `12345/678`, not
`1234/5678`. CurseForge's own wording is `.../files/{id}/{subId}/{filename}` `[docs]`
https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/

Reconstruction is **not** an auth bypass: it needs `fileName`, which only the
authenticated API gives you.

Requesting the reconstructed URL for the **blocked** EntityCulling file returns
**HTTP 302 → `https://mediafilez.forgecdn.net/files/8287/86/entityculling-forge-1.10.5-mc1.20.1.jar`**
— i.e. the file is still served. `[live]` Re-verified 2026-09-11: 302 then 200,
`application/java-archive`, **1,590,221 bytes = `fileLength`**, sha1/md5 byte-identical to the
API hashes, **and without an `x-api-key`** `[corrected 2026-09-11: live probe]`. Two side-notes from the
same probe: the blog's key-enforcement deadline was **July 16 2026** and is still not enforced
on this path; and a **ranged** `GET` (`Range: bytes=0-0`) on `mediafilez` returned **404**
while the plain `GET` returned 200 — **do not use range requests to probe availability.**

**Do not do this.** It deliberately defeats an author's opt-out that the API is
expressing, it is the exact behaviour that got third-party launchers into trouble, and
Prism explicitly does not do it (C.14). Documented here only so nobody "discovers" it
later and thinks it is a clever fix.

**Prism does not reconstruct CDN URLs.** Grepping the whole `develop` tree, `forgecdn`
appears only in `BuildConfig.h` (the header-proxy host constant) and two packwiz test
fixtures; there is no `/1000` or `%1000` arithmetic anywhere in the launcher. `[prism]`

**Prism's actual fallback chain for a blocked file — and it exists on ONE path only.**
`[corrected 2026-09-11: 2 refuters, whole-tree grep of the Prism launcher]` `FileResolvingTask` is
constructed **only** by `FlameInstanceCreationTask` (CurseForge modpack import) and
`FTBPackInstallTask` — i.e. the **modpack-import path**. The single-mod **browse/install path
never reaches it**: `ResourceAPI::getProjectVersions` drops any version with an empty
`downloadUrl` (C.2 step 2, line 119), so a blocked file is simply **absent from the version
combo box**. The **update** path only surfaces a website link (C.6 §3). Neither performs a
Modrinth sha1 lookup. `BlockedModsDialog` is likewise invoked by those two callers
(`FlameInstanceCreationTask.cpp:585`, `FTBPackInstallTask.cpp:218`), not by
`FileResolvingTask`. **Nebula should apply the fallback on *both* paths — that is a deliberate
superset of Prism, not a copy.** The chain itself `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FileResolvingTask.cpp:

1. `POST /v1/mods/files` with all `fileIds`.
2. For each file whose `downloadUrl` fails `QUrl::isValid()`, if it has a **sha1** hash,
   collect that hash.
3. **Ask Modrinth for the same file by sha1** and, if Modrinth has a byte-identical
   copy, download it from Modrinth instead (`qDebug() << "Found alternative on
   modrinth"`). **Gating detail** `[corrected 2026-09-11: 2 refuters, FileResolvingTask.cpp
   lines 133 and 150 + Application.cpp:901]`: the `POST /v2/version_files` request is sent
   **unconditionally** whenever any sha1 was collected (line 133); the setting
   `FallbackMRBlockedMods` (**default true**) gates only whether the **result is applied**
   (line 150), not whether the request is made.
4. Anything still unresolved → `BlockedModsDialog`, which opens the website link in a
   real browser and then hash-matches whatever the user drops in.

Step 3 is clever and worth stealing: many CF-blocked mods are also on Modrinth, and a
sha1 match proves it is the same artifact.

**The last-resort fallback** is what this repo already does `[repo]`
`crates/nebula-core/src/curseforge.rs:**179-181**` (line 177 is the
`counters.manual.push(ManualDownload {` opener — trivial off-by-two `[corrected 2026-09-11: repo read]`)
and what Prism does `[prism]`:
present a manual-download link

```
https://www.curseforge.com/minecraft/mc-mods/{slug}/download/{fileId}
```

built from `links.websiteUrl` (which is exactly
`https://www.curseforge.com/minecraft/mc-mods/{slug}`) plus `/download/{fileId}`.
Prism's phrasing: *"Resource has a new update available, but is not downloadable using
CurseForge."* `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameCheckUpdate.cpp

## B.6 Fingerprints — the CurseForge MurmurHash2 variant

### `POST /v1/fingerprints`

Body `{"fingerprints":[2298786945, 1]}` → HTTP 200 `[live]`. Response:

```jsonc
{"data": {
  "isCacheBuilt": true,
  "exactMatches": [ { "id": 238222, "file": {...full file object...}, "latestFiles": [...] } ],
  "exactFingerprints": [2298786945],
  "partialMatches": [],
  "partialMatchFingerprints": {},
  "installedFingerprints": [2298786945, 1],
  "unmatchedFingerprints": null
}}
```

**Trap verified live:** fingerprint `1` did not match anything, yet
**`unmatchedFingerprints` is `null`, not `[1]`.** `installedFingerprints` simply echoes
everything you submitted. `[live]`

**Therefore: compute the unmatched set yourself** as
`submitted − exactFingerprints`. Do not trust `unmatchedFingerprints`.

**`partialMatchFingerprints` — shape resolved, no longer UNVERIFIED.** `[corrected 2026-09-11:
3 refuters, swagger `components.schemas.FingerprintsMatchesResult`]` It is **not** a bare
object: the swagger declares
`{"type":"object","additionalProperties":{"type":"array","items":{"type":"integer","format":"int64"}}}`
— a **string-keyed map of `int64` arrays** (`Dictionary<string, long[]>`), mirroring the fuzzy
request's `foldername → fingerprints[]` grouping. docs.curseforge.com renders it as
`{"property1": [0], "property2": [0]}` with a `» additionalProperties [integer]` row. Only the
**key semantics** are undocumented; the shape is not. Live value with no partial matches is
`{}`. It is also the **fifth** field in both the schema and the live response order
(`isCacheBuilt`, `exactMatches`, `exactFingerprints`, `partialMatches`,
`partialMatchFingerprints`, `installedFingerprints`, `unmatchedFingerprints`), not the seventh
— as this section's own sample response shows. Model it as `HashMap<String, Vec<i64>>`.

There is also `POST /v1/fingerprints/{gameId}` (identical body and response), plus fuzzy
variants `POST /v1/fingerprints/fuzzy[/{gameId}]` which take a *different* body:
`{gameId, fingerprints: [{foldername, fingerprints: [int]}]}`. `[docs]` same.
**All four** fingerprint endpoints declare `200` / `400` / **`503 Service Unavailable`** —
`POST /v1/fingerprints`, `POST /v1/fingerprints/{gameId}`, `POST /v1/fingerprints/fuzzy` and
`POST /v1/fingerprints/fuzzy/{gameId}` `[corrected 2026-09-11: swagger `paths./v1/fingerprints*`]`.
The **503 body schema is a bare `string`**, not a JSON object — do not try to parse it as one.
Presumably tied to `isCacheBuilt: false`. Handle it. `[docs]`

Prism posts to the un-gamed `/v1/fingerprints` and types fingerprints as `QList<uint>`
— unsigned 32-bit, consistent with the algorithm below. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameAPI.h

**Wire encoding: the API accepts both numbers and strings; send numbers.** `[corrected 2026-09-11:
live probe + `FlameAPI.cpp` `matchFingerprints`]` Prism's C++ *parameter* is `QList<uint>` but
on the wire it serialises each value as a JSON **string**
(`fingerprintsArr.append(QString("%1").arg(fp));`). A live `POST` with
`{"fingerprints":["2298786945","1"]}` returned byte-for-byte the same response as the numeric
body. The swagger types the array as `int64`, so **numbers** are the canonical form — which is
also what step 5 of the algorithm below assumes.

### The algorithm — VERIFIED against a real file

MurmurHash2, 32-bit, little-endian, **over the whitespace-filtered bytes**:

1. Filter the file bytes, **dropping every byte equal to `0x09` (tab), `0x0A` (LF),
   `0x0D` (CR) or `0x20` (space)**. Filter over the *whole file as bytes*, not per line.
2. `m = 0x5bd1e995`, `r = 24`, `seed = 1`.
3. `h = seed ^ len` where **`len` is the FILTERED length**, not the file length.
4. Standard MurmurHash2 body over 4-byte little-endian words, then the 1–3 byte tail,
   then the final avalanche (`h ^= h >> 13; h *= m; h ^= h >> 15`).
5. Result is an **unsigned 32-bit** integer — it is sent as a JSON number and can exceed
   2^31 (EntityCulling's is `4018169999`).

**Known-answer test, executed 2026-09-11** `[live]`:

```
file            https://edge.forgecdn.net/files/8851/9/jei-1.20.1-forge-15.59.0.210.jar
file bytes      1825651
filtered bytes  1799143
computed        2298786945
API fileFingerprint  2298786945     MATCH
control seed=0  1738576881          (i.e. seed really is 1)
```

Use exactly this file/value pair as the unit test for the Rust implementation. The
filtered-vs-file length question — the one thing that is easy to get wrong — is settled:
**it is the filtered length.**

### Prism's implementation — and a licensing note in our favour

Prism reaches the same result via `Hashing::createHasher(path, FLAME)` →
`Algorithm::Murmur2`. Two files:

* `[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/helpers/HashUtils.cpp
  — the byte filter, literally
  `[](char c) { return (c == 9 || c == 10 || c == 13 || c == 32); }`.
* `[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/libraries/murmur2/src/MurmurHash2.cpp
  — the algorithm, with the comment `// This forces a seed of 1.` and
  `IncrementalHashInfo info{ (uint32_t)1 ^ size, (uint32_t)size };`.

**`libraries/murmur2/` is explicitly public domain, not GPL** — its header reads
*"MurmurHash2 was written by Austin Appleby, and is placed in the public domain… Those
modifications are also placed in the public domain."* So unlike the rest of Prism, the
constants there are safe to reference directly.

Prism computes `size` in a **first streaming pass** (counting non-filtered bytes), then
rewinds and streams again to hash — so it never holds the file in memory. It uses a 4 MiB
read buffer; that is an I/O detail and does not affect the digest.

**Implementation constraint — murmur2 cannot join the existing multi-hash pass.**
`crates/nebula-core/src/hash.rs` is **single-pass**: `digests_of_file` drives md5, sha1
and sha256 over one read, and its own comment says they were combined precisely because
*"a library jar is otherwise read twice."* `[repo]` murmur2 cannot be folded in, because
`h = seed ^ filtered_len` needs the filtered length **before the first mix**. So it is
necessarily either:

* a **separate two-pass read** (Prism's approach — count, rewind, hash), or
* **one pass that buffers the filtered bytes**, then hashes the buffer (simpler, but
  holds ~the file in memory; a 1.8 MB jar filters to 1.8 MB).

Since fingerprinting is only needed for CurseForge metadata recovery, not for every jar
on every scan, the two-pass read is the right trade. Do **not** widen the `Digests`
struct to hold it.

A corroborating fixture: `tests/testdata/Packwiz/screenshot-to-clipboard-fabric.pw.toml`
records `hash-format = "murmur2"`, `hash = "1781245820"` — a plain unsigned decimal
string. `[prism]`

## B.7 Icons

CurseForge serves `logo.thumbnailUrl` / `logo.url` from `media.forgecdn.net`, as
**JPEG or PNG** — the live JEI example is `.jpeg`. `[live]` **No webp**, unlike Modrinth
(A.6). `thumbnailUrl` is already resized (256×256 in the observed path) — always prefer
it over `url`.

---

# C. Prism Launcher behaviour

## C.0 The packwiz `.pw.toml` format — from packwiz upstream (not Prism)

Prism writes packwiz-format metadata, but packwiz itself is the authoritative and
**MIT-licensed** source for the schema, so this subsection is cited to packwiz rather
than to GPL Prism. Prism's specific *usage* of it — including five non-spec deviations — is in C.0b.

`[docs]` https://packwiz.infra.link/reference/pack-format/mod-toml/ and
`[docs]` https://github.com/packwiz/packwiz/blob/main/core/mod.go (Go structs with
literal `toml:` tags):

```go
type Mod struct {
    Name     string                            `toml:"name"`
    FileName string                            `toml:"filename"`
    Side     string                            `toml:"side,omitempty"`
    Pin      bool                              `toml:"pin,omitempty"`
    Download ModDownload                       `toml:"download"`
    Update   map[string]map[string]interface{} `toml:"update"`
    Option   *ModOption                        `toml:"option,omitempty"`
}
type ModDownload struct {
    URL        string `toml:"url,omitempty"`
    HashFormat string `toml:"hash-format"`
    Hash       string `toml:"hash"`
    Mode       string `toml:"mode,omitempty"`
}
type ModOption struct {
    Optional    bool   `toml:"optional"`
    Description string `toml:"description,omitempty"`
    Default     bool   `toml:"default,omitempty"`
}
```

Constants `[docs]` https://github.com/packwiz/packwiz/blob/main/core/mod.go:

```go
const ( ModeURL string = "url"; ModeCF string = "metadata:curseforge" )
const ( ServerSide = "server"; ClientSide = "client"; UniversalSide = "both"; EmptySide = "" )
```

* `download.mode` defaults to `"url"` when omitted/empty. `"metadata:curseforge"` means
  *the URL is not stored; resolve it from the CurseForge metadata in `[update.curseforge]`*
  — this is how packwiz handles files whose `downloadUrl` is null.
* `download.hash-format` allowed values: `sha256`, `sha512`, `sha1`, `md5`, `murmur2`.
  `[docs]` https://packwiz.infra.link/reference/pack-format/mod-toml/
* `update` is a nested table keyed by updater name:
  * `[update.modrinth]` → `mod-id` (string), `version` (string)
  * `[update.curseforge]` → `project-id` (number), `file-id` (number)
* Keys are **kebab-case**; use `#[serde(rename_all = "kebab-case")]`.
* Note `pin` (bool) — exists upstream; used to freeze a mod against updates.

Canonical shape:

```toml
name = "Sodium"
filename = "sodium-fabric-0.5.13+mc1.20.1.jar"
side = "client"

[download]
url = "https://cdn.modrinth.com/data/AANobbMI/versions/OihdIimA/sodium-fabric-0.5.13%2Bmc1.20.1.jar"
hash-format = "sha512"
hash = "<hex>"

[update.modrinth]
mod-id = "AANobbMI"
version = "OihdIimA"
```

CurseForge equivalent (distribution disallowed):

```toml
name = "Some Mod"
filename = "somemod-1.0.0.jar"
side = "both"

[download]
hash-format = "murmur2"
hash = "1234567890"
mode = "metadata:curseforge"

[update.curseforge]
file-id = 4567890
project-id = 238222
```

## C.0b (3) What Prism *actually* writes — five deviations from the packwiz spec

`[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/packwiz/Packwiz.cpp

**1. Prism adds five non-spec extension keys**, none of which are in packwiz:

```
x-prismlauncher-loaders          # array of loader strings
x-prismlauncher-mc-versions      # array of MC version strings
x-prismlauncher-release-type     # Unknown / Release / Beta / Alpha  <- CAPITALISED, four values
x-prismlauncher-version-number   # the provider's version_number
x-prismlauncher-dependencies     # array of tables: {addonId, type, version?}
```

**These are load-bearing for Prism itself**: the mod list's Loaders, Minecraft Versions
and Release Type columns are metadata-only and blank without them (C.11). **Prism never
determines a mod's loader by parsing the jar** — it comes solely from
`x-prismlauncher-loaders`.

**`x-prismlauncher-release-type` has FOUR capitalised values, not three lowercase ones.**
`[corrected 2026-09-11: Prism `ModIndex.h` L169-176, `EnumWrapper.h` L42-61, `Packwiz.cpp` L228
(write) / L303 (read)]` It is written via `IndexedVersionType::toString()`, whose mapping is
exactly `"Unknown"`, `"Release"`, `"Beta"`, `"Alpha"`. `EnumWrapper::fromString` compares
`str == name` **case-sensitively**, so a lowercase `release` emitted by another tool is read
back by Prism as `Unknown` (the `invalid()` value). Irrelevant to Nebula under D1 (JSON
index), but fatal to `.pw.toml` interop if that decision is ever revisited.

**2. `side` has FOUR legal values, not three.** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/ModIndex.h
adds `NoSide` → **the empty string `""`**, and `createModFormat` falls back to the
pack-level side which can be `NoSide`. A parser assuming `both|client|server` breaks.
Confirmed in the field: issue [#5262](https://github.com/PrismLauncher/PrismLauncher/issues/5262)
reports `side` blank in *every* index file across both providers and both loaders.
`[issue]`

**3. `hash-format` is never `murmur2` on the current write path.** It is copied verbatim
from the API parser: **Modrinth → `sha512` or `sha1`; CurseForge → `sha1` or `md5`**
(from the `algo` int, defaulting to sha1). If no hash entry matches, it is written
**empty**. Murmur2 is used for *fingerprint lookup only*, never for recording a download
hash. **But you must still accept `murmur2` on read** — packwiz itself and older Prism
versions write it (`hash = "1781245820"`, a decimal string). Read-side set:
**sha512, sha1, md5, murmur2.** `[prism]`

**4. `[download]` always writes all four keys** — `mode`, `url`, `hash-format`, `hash`.
For CurseForge, `mode = "metadata:curseforge"` and **`url` is written as an empty
string, not omitted**. `[prism]`

**5. `mode` is inert.** It is assigned, serialised, read back — and **nothing in the
entire tree ever branches on it.** It exists purely for packwiz interoperability.
`[prism]`

### Where the files live

`ResourceFolderModel::indexDir()` defaults to **`<resource dir>/.index`** — so
`mods/.index/`, `resourcepacks/.index/`, `coremods/.index/`, `datapacks/.index/`.
`[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/ResourceFolderModel.h

**Shaderpacks are the sole exception** — `ShaderPackFolderModel` overrides `indexDir()`
to return the folder itself, so shaderpack `.pw.toml` files sit **flat in
`shaderpacks/`**. There is a one-way `ShaderPackIndexMigrateTask` that moves them out of
`.index/` and deletes the directory; its failure path carries a
`// FIXME: not shown in the UI` comment. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/ShaderPackFolderModel.cpp

**On Windows Prism sets `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` on
any index dir whose name starts with `.`** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/tasks/LocalResourceUpdateTask.cpp
— because the folder scan filter is `QDir::Readable | QDir::NoDotAndDotDot | QDir::Files
| QDir::Dirs` with **no `QDir::Hidden`**. The dot prefix hides it on Unix; the attribute
is what hides it on Windows. **Nebula targets Windows — do the same, or filter `.index`
by name explicitly.** (The same code has an `else` branch resetting `FILE_ATTRIBUTE_NORMAL`,
commented *"fix shaderpacks folder being hidden by Prism Launcher 10.0.1"* — a shipped
regression where the wrong folder got marked hidden.)

Writing metadata at all is gated on the `ModMetadataDisabled` setting. `[prism]`

### Filename / slug derivation, and a wart

* The file is `<slug>.pw.toml`; `indexFileName()` is **idempotent** (returns the input
  unchanged if it already ends in `.pw.toml`).
* **The slug comes from the provider API's `slug` field, not from the filename.** If a
  fresh pack has an empty slug, the old metadata's slug is reused.
* **Wart:** the folder scan passes the raw directory entry (`"sodium.pw.toml"`) as the
  slug, so **metadata loaded from disk carries a slug that still has the `.pw.toml`
  suffix.** It only round-trips because `indexFileName()` is idempotent. Do not copy
  this; strip the suffix.
* Lookup is **case-insensitive with self-healing** — a case-insensitive directory scan
  on a miss, followed by renaming the file to the normalised casing. `[prism]`

### Read-side validation

* Missing `[download]` table → empty result, logs *"No [download] section found on mod
  metadata!"*
* `[update]` with neither `curseforge` nor `modrinth` → empty, *"No mod provider on mod
  metadata!"*
* **Type asymmetry that will bite you:** CurseForge `file-id` / `project-id` are read as
  **TOML integers**; Modrinth `mod-id` / `version` are read as **TOML strings**. Wrong
  type yields 0 / empty, not an error.
* Write side refuses to emit when IDs are missing. That guard exists because of a real
  bug — PR [#1543](https://github.com/PrismLauncher/PrismLauncher/pull/1543) fixed Prism
  emitting literally `file-id = ` with no value, i.e. syntactically invalid TOML, and
  made one corrupt file stop aborting the whole metadata load. `[issue]`
* `updateModIndex` **deletes and rewrites the whole file** rather than merging.

**On-disk key ordering is UNVERIFIED** — Prism uses external toml++, and no test asserts
written output. Do not depend on key order.

### `MetadataHandler` — the API surface, and the O(n) lookup

`[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/MetadataHandler.h
is a **pure inline forwarding shim** — no state, no storage decisions of its own.
`namespace Metadata`, with `using ModStruct = Packwiz::V1::Mod`:

| Function | Forwards to |
| --- | --- |
| `create(index_dir, pack, version)` | `Packwiz::V1::createModFormat` |
| `update(index_dir, mod)` | `Packwiz::V1::updateModIndex` |
| `remove(index_dir, slug)` | `Packwiz::V1::deleteModIndex` |
| `get(index_dir, slug)` | `Packwiz::V1::getIndexForMod` (by slug — direct file open) |
| `get(index_dir, mod_id)` | `Packwiz::V1::getIndexForMod` (by project id) |

**Every function takes `index_dir` explicitly — the handler stores nothing.**

**The by-ID lookup is O(n): it parses every `.pw.toml` in the directory until one
matches.** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/packwiz/Packwiz.cpp

Lookup by *slug* is O(1) (the slug is the filename); lookup by *project id* — which is
what dependency resolution and update checking actually need — is a full directory parse.
**This is the concrete cost behind decision D1.** With 200 mods, every "do I already have
project X?" question parses 200 TOML files. Prism gets away with it because the
dependency task loads all metadata once up front into `m_mods`; a naive per-query
implementation would be quadratic.

## C.1 Snapshot and structural corrections

All Prism citations are branch `develop` @ `0db8771a2805b21a10721bdfd2364757f26cd369`
(2026-09-10), `Launcher_VERSION_MAJOR = 12`. **Several files named in common write-ups
have moved or been deleted on `develop`** — if you go looking, note:

| Commonly cited | Reality on `develop` |
| --- | --- |
| `Modrinth::loadIndexedPackVersions` (plural) | gone — filtering/ordering moved into `ResourceAPI::getProjectVersions` |
| `NetworkResourceAPI.*` | does not exist; its role is `ResourceAPI.cpp` |
| `ModUpdateDialog` | renamed `ResourceUpdateDialog` |
| `modplatform/GetModDependenciesTask` | lives at `launcher/minecraft/mod/tasks/GetModDependenciesTask.cpp` |
| "CF update check uses fingerprints" | **false** — fingerprints are used only for metadata *recovery* (C.6) |

Base URLs `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/buildconfig/BuildConfig.h:
Modrinth `https://api.modrinth.com/v2`, CurseForge `https://api.curseforge.com/v1`,
`FLAME_DOWNLOAD_HOST = "edge.forgecdn.net"`.

User-Agent is `PrismLauncher/12.0.0` (`Launcher_CommonName/Launcher_VERSION_NAME`), set
on every request, overridable via the `UserAgentOverride` setting with `$LAUNCHER_VER`
substitution. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/Application.cpp

`ApiHeaderProxy` adds, host-matched `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/net/ApiHeaderProxy.h:
`x-api-key` for CF API + CDN hosts; `Authorization` for Modrinth when a token is set; and
on Modrinth **CDN** downloads a `modrinth-download-meta` header carrying
`{"reason":"standalone"|"update"|"dependency", "game_version":…, "loader":…, "dependent_on":…}`.
That last one is a courtesy telemetry header — worth mirroring so Modrinth's download
stats stay meaningful.

## C.2 (1) How Prism picks the version to install

`ResourceAPI::getProjectVersions` is provider-agnostic `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/ResourceAPI.cpp:

1. Parse every entry (`data` array for CF, bare array for Modrinth).
2. **Drop anything with an invalid `fileId` or an empty `downloadUrl`.** This is what
   silently hides CurseForge blocked mods from the browse list.
3. **Re-sort client-side, descending, by plain string comparison on the date field**
   (`a.date > b.date`, RFC 3339). **Prism does not trust API ordering** — not
   Modrinth's, not CurseForge's.

The UI walks that list in order, keeps versions passing `ModModel::checkVersionFilters`,
and **combo-box index 0 — the newest survivor — is the default.** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/modplatform/ResourcePage.cpp

`checkVersionFilters` requires: not opted-out; loaders unset on either side or bitwise
intersecting; side compatible; release type in the filter set **if that set is
non-empty**; MC version in the filter set if non-empty. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/modplatform/ModModel.cpp

### Does it prefer release over beta/alpha? **No.**

The Release/Beta/Alpha/Unknown checkboxes in `ModFilterWidget.ui` carry **no `checked`
property**, so the release-type filter set starts empty, and empty means "accept
everything". `prepareBasicFilter` sets loaders, MC version, side, `hideInstalled` and
`openSource` — **it never touches release types.** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/widgets/ModFilterWidget.cpp

**Net effect: the newest file wins even if it is an alpha.** Release type is display-only
decoration (`" [Release]"` etc. in `getVersionDisplayString`).

This is observable in CurseForge's live data: the newest 1.20.1 Forge JEI file
(`jei-1.20.1-forge-15.59.0.210.jar`, `fileDate 2026-09-10`) is `releaseType: 2` **Beta**
(B.4) — Prism would install it. `[corrected 2026-09-11: 3 refuters — **the Modrinth Sodium half of
this sentence was false** and has been removed: live, Sodium's newest 1.20.1 version is the
`0.5.13` **release** (2025-03-03) and the `0.5.12-beta.2` beta is older (2025-02-26), so
`array[0]` is the release and Prism's newest-wins rule yields the right answer there. See the
corrected note in A.4.]`

**Recommendation for Nebula: do not copy this.** Default to "newest **release**, fall
back to newest of any type if no release matches", and expose the checkboxes pre-checked
on Release. This is a small deliberate divergence and it is the right one for a
server-provisioning tool.

## C.3 The "primary file" rule — and a real bug

`Modrinth::loadIndexedPackVersion` `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/modrinth/ModrinthPackIndex.cpp
loops `while (i < files.count() - 1)` and breaks on:

1. first file whose **filename contains** `preferredFileName` (a *substring* match; in
   the update path this is the loader string, e.g. `"fabric"`), else
2. first file with `primary: true`, else
3. **the LAST file** — because the loop bound is `count() - 1`, `i` is left at
   `count()-1` when nothing matches. The in-source comment says exactly this.

**Known defect worth not reproducing:** immediately after the loop, `isPreferred` is
unconditionally reassigned:

```
file.isPreferred = Json::requireBoolean(parent, "primary") || (files.count() == 1);
```

This **clobbers** the `isPreferred = true` set by branch 1. Since `isPreferred` gates
whether an update is offered (C.6), **a version whose loader-specific file is not flagged
`primary` resolves correctly for download but is never surfaced as an available update.**

For Nebula: rule 3 (fall back to the *last* file) is wrong — **and so was the replacement
this document originally proposed.** `[corrected 2026-09-11: 3 refuters — `file_type` is null on
sources jars too (A.4), so "first file with `file_type == null`" would happily pick
`…-sources.jar`]` The correct fallback chain is:

1. the file with `primary == true`;
2. else the **first** file whose `filename` ends `.jar` and does **not** end `-sources.jar`,
   `-dev.jar` or `-javadoc.jar` (name-based, because `file_type` is unreliable);
3. else the first file at all, and log it.

And never let the "preferred" flag be recomputed after selection.

Hash selection: Modrinth prefers `sha512`, else the first of `{sha512, sha1}` present.
CurseForge takes the **first array entry** whose algo is in `{sha1, md5, murmur2}` — i.e.
**response order decides, not preference order**. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/ModIndex.cpp

## C.4 Loader handling, and the 1.20.1 NeoForge special case

Internal bitfield `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/ModIndex.h —
**NeoForge is bit 0, not Forge** (this changed from older releases; do not assume
Forge=1):

```
NeoForge 1<<0   Forge 1<<1   Cauldron 1<<2   LiteLoader 1<<3
Fabric   1<<4   Quilt 1<<5   DataPack 1<<6   Babric   1<<7
BTA      1<<8   LegacyFabric 1<<9  Ornithe 1<<10  Rift 1<<11
```

* **To Modrinth:** loader strings `neoforge, forge, fabric, quilt, liteloader, datapack,
  babric, bta-babric, legacy-fabric, ornithe, rift`. **Cauldron is never sent.**
  Multiple loaders go out at once — OR'd `categories:` facets in search, a JSON array in
  `loaders=[...]` for version listing. `[prism]`
* **To CurseForge:** `Forge 1, Cauldron 2, LiteLoader 3, Fabric 4, Quilt 5, NeoForge 6`;
  anything else → 0, and **0 is never sent** — it means "no filter", so Prism drops the
  parameter. That is the right behaviour and not merely a style choice: `modLoaderType=0` on
  the wire returns **zero results**, it is not a wildcard (B.2) `[corrected 2026-09-11: 2 refuters]`.
  Search takes `modLoaderTypes=[1,4]`. **Version listing takes only one**, and only when
  exactly one bit is set (`hasSingleModLoaderSelected`, a popcount-1 check) — otherwise the
  parameter is **omitted entirely** and filtering falls to the client. `[prism]`

### Yes — Forge/NeoForge on 1.20.1 is explicitly special-cased

`PackProfile::getSupportedModLoaders()` `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/PackProfile.cpp:

```cpp
if (loaders & ModPlatform::Quilt) loaders |= ModPlatform::Fabric;
if (getComponentVersion("net.minecraft") == "1.20.1" && (loaders & ModPlatform::NeoForge))
    loaders |= ModPlatform::Forge;
```

* Both rules are **one-directional**: a NeoForge instance on exactly `"1.20.1"` also
  accepts Forge mods; a Forge instance never gains NeoForge. Quilt→Fabric is
  unconditional.
* The MC match is **exact string equality against `"1.20.1"`** — not a range, not a
  prefix.
* Consequence: on a 1.20.1 NeoForge instance the loader set has two bits, so
  `hasSingleModLoaderSelected` is false and Prism **omits `modLoaderType` from the CF
  file listing entirely**, filtering client-side.

This answers section A's open question: Modrinth does not alias `forge`/`neoforge`
(A.7); **Prism does the widening itself, client-side, and only for 1.20.1.**

Prism also normalises MC version strings both ways for Modrinth: `" Pre-Release "` ⇄
`-pre`, spaces ⇄ `-`, storing **both** forms so filters match either. `[prism]`

## C.5 (2) Dependency resolution

`GetModDependenciesTask`, a `SequentialTask` `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/tasks/GetModDependenciesTask.cpp:

**Which types are followed:** **only `REQUIRED`.** `OPTIONAL`, `INCOMPATIBLE`,
`EMBEDDED`, `TOOL`, `INCLUDE`, `UNKNOWN` are all skipped — **no warning, no UI surface**.
Notably `INCOMPATIBLE` is silently ignored rather than flagged as a conflict.

**Seeding.** Each user-selected pack must pass `checkDependencies` (its version's
`mcVersion` must contain the instance MC version, or the pack has no versions at all;
and loaders must be unset on either side or intersect). Passing selections enqueue their
required deps at **level 20**.

**Deduplication — four checks, all before dispatch:** already in the current batch;
already in `m_selected`; already an installed mod with matching **provider + project
id**; already in `m_packDependencies`. **All are provider-scoped** — a Modrinth
dependency is never considered satisfied by an installed CurseForge mod.

**Cross-provider mapping: there is none.** The only remap, `getOverride`, stays inside
one provider, fires only on Quilt/Fabric instances, and covers exactly **two hardcoded
projects** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/ModIndex.h:

| Project | Quilt id | Fabric id | Provider |
| --- | --- | --- | --- |
| API | `634179` | `306612` | CurseForge |
| KotlinLibraries | `720410` | `308769` | CurseForge |
| API | `qvIfYCYJ` | `P7dR8mSH` | Modrinth |
| KotlinLibraries | `lwVhp9o5` | `Ha28R6CL` | Modrinth |

On a Quilt instance a dep on the Fabric id is rewritten to the Quilt id, and vice versa
on a Fabric instance. **Nothing maps a Modrinth project to a CurseForge project, by hash
or otherwise.**

**Resolving each dependency** (`ResourceAPI::getDependencyVersion`): if a Modrinth
`version_id` is pinned, `GET /v2/version/{id}` directly; otherwise the provider's normal
version listing filtered by MC version + loaders. Keep entries with a valid `fileId`
**and** matching loaders, sort by date descending, **take `front()`**.

**No matching version → the dependency is dropped silently.** `removePack(addonId)`,
with only a `qCritical` on network failure. There is **no user-facing "unresolvable
dependency" error.** (The one exception: on a Quilt instance a failed override id is
retried once with the Fabric id at the same level.)

**Recursion depth: `level` starts at 20 and decrements per hop.** The check
(`if (level == 0) { removePack; qWarning() << "Dependency cycle exceeded"; return; }`)
runs **after** resolution, so chains deeper than 20 are truncated with a log line only.

**Local-install suppression** uses `laxCompare` on the **filename**: lowercase, then
`-`, `+`, `.`, `_` → space, whitespace simplified, exact compare. A match →
`removePack`, and **recursion stops there** (transitive deps of an already-present mod
are never explored). An empty filename also counts as installed.

**`maybeInstalled`** re-runs `laxCompare` with `excludeDigits = true` (digits treated as
separators too) — a version-number-insensitive filename match. A hit **pre-unchecks** the
row rather than hiding it, so the user can still opt in. This also builds
`requiredByNames` / `requiredByIds` for the UI, and `requiredByIds.first()` becomes the
`dependent_on` field of the Modrinth download-meta header.

**Kill switch:** the whole step is skipped when the `ModDependenciesDisabled` setting is
true. `[prism]`

## C.6 (5) Update checking

Entry point: `ResourceUpdateDialog(..., includeDeps, profile->getModLoadersList())` —
the loader list including the widened Fabric/Forge entries from C.4. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/instance/ModFolderPage.cpp

**Step 0 — metadata backfill.** `ensureMetadata()` runs first. Mods without metadata go
through `EnsureMetadataTask` against their apparent provider; **on failure it retries
with the other provider** (`next()` flips MODRINTH ↔ FLAME). `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/EnsureMetadataTask.cpp

**This is the only place `POST /v1/fingerprints` is used.** Local files are murmur2-hashed
and matched against `data.exactMatches[].file.fileFingerprint`. Modrinth's equivalent is
`POST /v2/version_files` with **sha512** hashes. Failures are pooled into a
`ScrollMessageBox` — *"Do you wish to proceed without those resources?"* — and rejecting
aborts the whole dialog.

Hashing runs as a `ConcurrentTask` bounded by `NumberOfConcurrentTasks`; **folders are
skipped** entirely. `[prism]`

**On write**, `updateMetadata()` overrides the API's filename with the real on-disk one
**and chops a trailing `.disabled` (9 chars)** — so the `.pw.toml` always records the
*enabled* filename even for a currently-disabled jar. It then re-reads the file back off
disk to confirm it parses before marking the resource ready. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/EnsureMetadataTask.cpp

**On failure, no `.pw.toml` is written and no negative-result marker is left.** The jar
simply stays `NoMetadata` and is **re-hashed and re-looked-up on every subsequent
attempt.** `[prism]`

> **Design input, not trivia:** a mods folder containing a handful of jars that exist on
> neither provider (hand-built mods, private builds, renamed jars) pays a **full murmur2
> pass over each of them on every launch, forever.** Cache the negative result keyed on
> `(sha512, murmur2)` with a TTL — the hash is already computed, so the cache entry is
> free.

Provider fallback lives in the **caller**, not the task: `ResourceUpdateDialog` runs
Modrinth first, collects `metadataFailed` signals, and retries that subset against
CurseForge. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/dialogs/ResourceUpdateDialog.cpp

### Modrinth `[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/modrinth/ModrinthCheckUpdate.cpp

1. `m_hashType = "sha512"`. Any resource whose stored `hash_format` differs is
   **re-hashed on disk with sha512**, concurrently, bounded by the
   `NumberOfConcurrentTasks` setting.
2. Loader list = instance loaders, then the union of the mods' own recorded
   `metadata()->loaders` minus those already present.
3. **Iterate loaders one at a time** — `POST /v2/version_files/update` with
   `{"hashes":[...], "algorithm":"sha512", "loaders":["fabric"], "game_versions":["1.20.1"]}`.
   `loaders` / `game_versions` are omitted when absent. **Prism does not batch loaders
   into one request.**
4. **On the wire the hash is simply ABSENT from the response map** — the API never returns
   an empty object for it. `[corrected 2026-09-11: 2 refuters, live probes + labrinth
   `routes/v3/version_file.rs` (`response.entry(hash).or_default().push(..)` only runs on a
   match) + `ModrinthCheckUpdate.cpp` L140-148]` The "empty object" is an artefact of Prism's
   `doc[hash].toObject()`, which yields an empty `QJsonObject` for a missing key — that is
   what `projectObj.isEmpty()` catches. "The map" that retains the entry for the next loader
   is **Prism's own `m_mappings`**, not the API response. **A Rust client must treat
   "key absent" as the no-match signal — never look for `{}` or `null`.**
5. Empty `downloadUrl` → `qCritical`, skip, retry next loader.
6. **"An update exists" iff
   `(projectVer.hash != submittedHash && projectVer.isPreferred) || status == NotInstalled`.**
   The same-version case falls out naturally: an identical hash means no update. The
   `isPreferred` conjunct is the source of the C.3 clobbering bug.
7. Leftovers emit *"No valid version found for this resource. It's probably unavailable
   for the current game version / mod loader."*

### CurseForge `[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/modplatform/flame/FlameCheckUpdate.cpp

**No fingerprint call here.** One `GET /v1/mods/{project_id}/files?pageSize=10000&gameVersion=<instance MC>`
per resource, all in a single `NetJob`.

1. Parse `data`, keeping everything with a valid `fileId` — **including files with an
   empty `downloadUrl`**, unlike the browse path — and sort by `fileDate` descending.
2. `FlameAPI::getLatestVersion` buckets versions by loader (a version with no loaders
   goes to a sentinel `ModLoaderType(0)` bucket; a multi-loader version goes in every
   matching bucket), keeping the max-date per bucket. Preference order: **instance
   loaders → the mod's own previously recorded loaders → the no-loader sentinel.** The
   in-source comment: *"edge case: mod has installed for forge but the instance is fabric
   ⇒ fabric version will be prioritizated on update"*. One tiebreak: if the chosen bucket
   is a real loader, a no-loader bucket also exists, and there are **exactly two** buckets
   total, the no-loader entry wins if newer.
3. **Blocked files:** empty `downloadUrl` **and** a file id differing from the installed
   one → parked in `m_blocked`; `collectBlockedMods()` then fetches project info (single
   `getProject`, or batch `POST /v1/mods`) and emits *"Resource has a new update
   available, but is not downloadable using CurseForge."* with a
   `<websiteUrl>/download/<fileId>` recovery link. A blocked file whose id **equals** the
   installed one is treated as "no update".
4. **"An update exists" iff
   `!latestVer->hash.isEmpty() && (metadata()->hash != latestVer->hash || status == NotInstalled)`.**
5. Changelogs are fetched per update via `GET /v1/mods/{id}/files/{fileId}/changelog`
   — **synchronously, blocking on a nested `QEventLoop`.** Do not copy this; it is a
   known UI-freeze pattern.

### Presentation

Both checks run in one `SequentialTask` under an abortable `ProgressDialog`. Results go
into a `ReviewMessageBox` tree; the dependency pass then runs on the combined `m_deps`
and appends rows labelled *"Not installed"*, pre-unchecked when `maybeInstalled`. All
failures pool into a `ScrollMessageBox` offering a **"Disable unavailable mods"**
checkbox that calls `mod->enable(EnableAction::DISABLE)` on each. An empty tree reports
*"All mods are up-to-date! :)"*. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/dialogs/ResourceUpdateDialog.cpp

## C.7 (8) Download filenames and hash verification

**Filename:** `ResourceDownloadTask::getFilename()` returns `m_pack_version.fileName`
**verbatim** — the provider's own filename (Modrinth `files[i].filename`, CurseForge
`fileName`), passed through `FS::RemoveInvalidPathChars()` at parse time. No renaming, no
version suffix, no hash in the name. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ResourceDownloadTask.h

This is the correct side of the A.4 `+`-encoding trap: the **filename** field is used for
disk, the **url** field for the request.

**Hash verification: yes, and it gates the write.** `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ResourceDownloadTask.cpp

* A `Net::ChecksumValidator` is attached when both `hashType` and `hash` are non-empty,
  mapping md4/md5/sha1/sha256/sha512 to `QCryptographicHash`.
* **`murmur2` hits `default: break` — no validator is attached**, so a murmur2-only
  version downloads **unverified**. (In practice CF always supplies sha1 and md5.)
* `ChecksumValidator` hashes incrementally during `write()` and compares in `validate()`.
  **An empty expected value passes trivially.**
* Ordering is correct and worth copying: `FileSink` writes to an atomic
  temp-then-commit `PSaveFile`, and `finalize()` runs the validators **before**
  `commit()`. **A hash mismatch therefore fails the task and the temp file is never
  promoted — a corrupt jar cannot land in the mods folder.** `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/net/FileSink.cpp

Old-file cleanup is deferred to `downloadSucceeded()` and is **skipped when the filename
is unchanged**. `[prism]`

## C.8 Cross-cutting notes

* Ordering is **always newest-first by publish date**, computed client-side by string
  comparison on the RFC 3339 timestamp, in all three places that pick a version
  (`getProjectVersions`, `getDependencyVersion`, `FlameMod::loadIndexedPackVersions`).
  String comparison is only correct because both APIs return `Z`-normalised timestamps —
  parse properly rather than relying on that.
* **Release type never influences ordering or selection anywhere in Prism.**
* Modrinth search facets use **`categories:`** for loaders — the single most common
  reimplementation mistake.
* Provider hash preferences: Modrinth `{sha512, sha1}`; CurseForge `{sha1, md5,
  murmur2}` for download verification, **murmur2 only** for local fingerprinting.
* `ProviderCapabilities::name()` → `"modrinth"` / `"curseforge"`; `getMetaURL` builds
  `https://modrinth.com/mod/<id>` and `https://www.curseforge.com/projects/<id>`.
  `[prism]`
* The two "is this already installed?" tests are deliberately different and both matter:
  **hash-exact** for updates, **`laxCompare` on filename** for dependencies.

## C.9 (4) Local jar parsing — `LocalModParseTask`

`[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/tasks/LocalModParseTask.cpp

### There is no precedence list and no merging

**The single most important correction.** `processZIP` makes **one sequential streaming
pass** over the archive (libarchive's `archive_read_next_header` loop). Each handler
**assigns the details struct wholesale** — clobbering anything earlier — and sets
`stop = true`.

* **Precedence = whichever metadata file appears first in the jar's byte stream.** Not
  central-directory order. Not a priority list.
* **Nothing is ever merged.** There is no "read `fabric.mod.json`, fill gaps from
  `MANIFEST.MF`".
* **Sole exception:** a `mods.toml` whose `version` is literally `${file.jarVersion}`
  sets `stop = false` and keeps scanning until `META-INF/MANIFEST.MF`, then substitutes.

Recognised entries: `META-INF/mods.toml`, `META-INF/neoforge.mods.toml`,
`META-INF/MANIFEST.MF`, `mcmod.info`, `quilt.mod.json`, `fabric.mod.json`,
`forgeversion.properties`, `META-INF/nil/mappings.json` + `*.nilmod.css`.

**`mods.toml` and `neoforge.mods.toml` hit the identical branch** — NeoForge support was
added as a one-line `||` in PR [#2319](https://github.com/PrismLauncher/PrismLauncher/pull/2319).
A jar shipping both gets whichever comes first, parsed identically. `[issue]`

**For Nebula: do not copy the byte-order rule.** Read the whole central directory, then
apply an explicit precedence (`neoforge.mods.toml` → `mods.toml` → `fabric.mod.json` →
`quilt.mod.json` → `mcmod.info` → `MANIFEST.MF`) and merge missing fields. `zip 8.6.0`
is already a dependency and gives random access by name `[repo]`.

### `fabric.mod.json`

* **Unconditional:** `id` → mod id, `version`, `name` (**falls back to `id`**),
  `description`.
* **Everything else is gated on `schemaVersion >= 1`.** A file with no `schemaVersion`
  (defaults to 0) yields *only* those four — **no authors, no contact, no license, no
  icon.**
* `authors[]` — each element may be a **string** or an **object read via `.name`**.
* `contact.homepage` → home URL, `contact.issues` → issue tracker.
  **`contact.sources` is NOT read.**
* `license` — accepts a string, an object (`name`/`id`/`url`/`description`), **or an
  array** of either.
* **`icon` — confirmed: string path OR size-keyed object.** For the object form Prism
  splits each key on `'x'`, parses the leading part as an integer, and **keeps the value
  for the largest**. If every key parses to ≤ 0 it falls back to the **first** entry.

### `quilt.mod.json`

Gated on `schema_version == 1` **exactly**. Nesting: `quilt_loader.id`,
`quilt_loader.version` (both **required** — a throw is caught and logged, yielding empty
details), then `quilt_loader.metadata.{name, description, contributors, contact, license,
icon}`. `metadata.name` falls back to the mod id. **`metadata.contributors` is an object
and Prism takes its KEYS as the author list**, discarding the roles. `depends` is read
from the **top-level** object, not from `quilt_loader`.

**Quilt's icon selection is subtly broken and differs from Fabric's.** Fabric tracks the
winning *value* while scanning; Quilt tracks only the largest *number*, then
**reconstructs the key as `"<n>x<n>"` and looks it up**. So an object keyed `"128"`
rather than `"128x128"` parses the size correctly, fails the lookup, and silently yields
no icon. Use Fabric's approach for both.

### `META-INF/mods.toml` / `neoforge.mods.toml`

**The precedence is asymmetric, and `logoFile` is the opposite of the common guess:**

| Read from | Keys |
| --- | --- |
| **`[[mods]][0]` only** | `modId`, `version`, `displayName`, `description` |
| **TOP-LEVEL first, `[[mods]][0]` as fallback** | `authors`, `displayURL`, `issueTrackerURL`, `license`, **`logoFile`** |

**For `logoFile`: top level wins; per-mod is the fallback.** `[prism]`

* **Only `[[mods]][0]` is ever read** — multiple mods in one file is unsupported by
  design. A missing `[[mods]]` array logs *"Corrupted mods.toml? Couldn't find [[mods]]
  array!"*.
* **`modLoader` is never read anywhere in the tree.** Neither is `loaderVersion`.
* `authors` is treated as a single **string** appended to the list, not an array.
* `dependencies` handles both the Forge shape (`mandatory = true`) and the NeoForge
  shape (`type = "required"`).
* `displayURL` gets `http://` prepended if it has no scheme.

**`${file.jarVersion}` resolution:** from `META-INF/MANIFEST.MF`, matching
`Implementation-Version:` **case-insensitively**. A value containing
`task ':jar' property 'archiveVersion'` (Gradle leaking) or an empty value both become
the literal string **`"NONE"`**, matching Forge's own behaviour. `[prism]`

### `mcmod.info` (legacy Forge)

Accepts a bare top-level array, or an object with `modinfoversion` / `modListVersion` and
a `modlist` / `modList` array. A version other than 2 warns but **still parses**.

* **Read:** `modid` (leading `mod_` stripped), `name`, `version`, `url`, `description`,
  `logoFile`, `authorList` **falling back to** `authors`, `requiredMods` **falling back
  to** `dependencies`.
* **NOT read:** `credits`, `updateUrl`, `mcversion`, `parent`, `screenshots`.
* A `name` of exactly **`"Example Mod"` is discarded** as a copy-paste artifact.

### `META-INF/MANIFEST.MF`

**Only `Implementation-Version`**, and only as version material. It is not a general
metadata fallback — a jar with nothing but a manifest yields `isValid == false`.

### Icon extraction — `processIconPNG` is misnamed

It is **`QImage::fromData(raw_data)`** — **content-sniffing and format-agnostic**, with
no extension check and no format whitelist. Whatever `QImageReader` plugins are
registered will decode it. The error strings all say *"invalid png image"* regardless of
actual format, which misleads anyone debugging from logs. `[prism]`

`Mod::setIcon` scales to **64×64** with `KeepAspectRatioByExpanding + SmoothTransformation`
— the cause of open issue
[#2195](https://github.com/PrismLauncher/PrismLauncher/issues/2195), *intentionally
pixelated mod icons render blurry.* `[issue]` **For a pixel-art icon use nearest-neighbour.**

**There is no on-disk cache for jar-extracted icons.** On every `QPixmapCache` eviction
the jar is re-opened and re-scanned linearly. `[prism]`

## C.10 (6) Enable / disable, and deletion

`[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/Resource.cpp

**It is an append, not a replacement:** `foo.jar` → **`foo.jar.disabled`**. Enabling
requires the path to end with `.disabled` and **chops exactly 9 characters**. One
`QFile::rename` — but **the disable direction has a collision branch the original text
missed.** `[corrected 2026-09-11: 2 refuters, `Resource.cpp` `enable()` ~L257-272 and
`FileSystem.cpp` `getUniqueResourceName` L1714-1736]`

`Resource::enable(DISABLE)` does `path += ".disabled"; if (QFile::exists(path)) path =
FS::getUniqueResourceName(path);`. When a stale `foo.jar.disabled` twin already exists,
`getUniqueResourceName` chops the `.disabled`, sees that the **source** `foo.jar` (not yet
renamed) still exists, and returns `<completeBaseName>.duplicate` — so the *live* jar is
renamed to **`foo.jar.duplicate`**, which rescans as a `SINGLEFILE` rather than as a disabled
mod, while the stale `.disabled` file is left untouched. Still one rename; the target name is
just not what you would predict. (Nebula's namespace-directory model, D1, sidesteps this
entirely — recorded so nobody re-derives the `.disabled` scheme and inherits the bug.)

* Detection on scan: **only if the entry is a file**. A *directory* named `foo.disabled`
  is **not** treated as disabled.
* **`ResourceType` has five values:** `UNKNOWN`, `ZIPFILE` (`.zip`/`.jar`/`.nilmod`),
  `SINGLEFILE` (any other file — full filename kept), `FOLDER`, `LITEMOD` (`.litemod`).
* **Folders cannot be toggled** — `enable()` returns `false` immediately for `FOLDER`
  and `UNKNOWN`. The checkbox still renders and is still clickable; **the toggle
  silently fails.**
* **Rename failure is silent** — returns false, state unchanged, no dialog, no log.
* **Toggling never touches the `.pw.toml`.** After disabling, the metadata still records
  `filename = "foo.jar"` while disk has `foo.jar.disabled`. The scan reconciliation
  (C.12) is what papers over the mismatch.
* Mods get **dependency-aware toggling**: a `"Confirm toggle"` dialog offering *"Toggle
  Required Mods"* vs *"Only Toggle Selected"*. `[prism]`
  https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/ModFolderModel.cpp

**Both twins present** (`foo.jar` *and* `foo.jar.disabled`) is handled by
`FS::getUniqueResourceName`, called on **every** entry during the scan, which physically
**moves files**: it prioritises the enabled one and renames the disabled twin to
`<completeBaseName>.duplicate`, then `.duplicate2`… Since `completeBaseName()` of
`foo.jar.disabled` is `foo.jar`, the result is literally **`foo.jar.duplicate`** — which
then re-parses as a `SINGLEFILE`. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/FileSystem.cpp

**Deletion order:** set type UNKNOWN → **delete the `.pw.toml` first** (unless
`preserveMetadata`) → `(attemptTrash && FS::trash(path)) || FS::deletePath(path)` —
trash first, permanent delete as short-circuit fallback. `FS::trash` returns false
unconditionally on Windows Server. **Fragile path:** with no metadata,
`destroyMetadata` falls back to `Metadata::remove(indexDir, name())` using the
**human-readable display name**, so it looks for `Just Enough Items.pw.toml` and normally
finds nothing. `deleteResources()` **returns true unconditionally**, ignoring per-resource
failures. `[prism]`

## C.11 What the mod list displays

14 columns `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/ModFolderModel.h:
`Enable`, `Image`, `Name`, `Version`, `Last Modified`, `Provider`, `Size`, `Side`,
`Loaders`, `Minecraft Versions`, `Release Type`, `Requires`, `Required By`, `File Name`.
**Most are hidden by default** — only `Image`, `Version`, `Last Modified`, `Provider`,
`Pack Format`, plus the non-hideable `Enable` and `Name`.

**Name fallback chain — the opposite of intuitive:**
**(1) jar-parsed name → (2) `.pw.toml` `name` → (3) filename with suffixes stripped.**
Because `details()` is empty until the async parse finishes, **the displayed name visibly
changes** from the metadata name to the jar name a moment after the list loads. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/Mod.cpp

* **Version** is `details().version` **flat, with no metadata fallback** — a jar with a
  `.pw.toml` but unparseable jar metadata shows a **blank** version.
* **Provider is never blank:** `"Unknown"` / `"Modrinth"` / `"CurseForge"`. The decision
  is purely *"does a `.pw.toml` exist and what does its `[update]` name"* — **no jar
  content sniffing at all.**
* **Side** falls back to `"both"` (not blank) with no metadata, and emits the **raw
  untranslated lowercase** string.
* **Loaders / Minecraft Versions / Release Type are metadata-only** (they come from the
  `x-prismlauncher-*` keys, C.0b). **Loaders and Minecraft Versions are blank** without a
  `.pw.toml` (`return {}`), but **Release Type is not** — `Mod::releaseType()` falls back to
  `IndexedVersionType(Unknown).toString()` and therefore displays the literal string
  **`"Unknown"`** `[corrected 2026-09-11: Prism `Mod.cpp` L207-247]`.
* **File Name includes the `.disabled` suffix** when disabled.

## C.12 Scanning and reconciliation

A `QFileSystemWatcher` watches **both** `.index` and the resource dir. `ResourceStatus`
drives everything: `Installed` (jar + metadata), `NotInstalled` (metadata only),
`NoMetadata` (jar only), `Unknown`. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/mod/tasks/ResourceFolderLoadTask.cpp

1. **Metadata first:** every valid `*.pw.toml` creates a placeholder **at the path the
   metadata claims**, marked `NotInstalled`.
2. **Then the directory**, after `getUniqueResourceName` de-duplication. Enabled entries
   matching a placeholder upgrade it to `Installed`. **Disabled entries chop 9 chars and
   look up the enabled name** — on a match they inherit the metadata. *That is exactly
   how a disabled jar keeps its provider/side/loaders despite the stale `filename` in the
   toml.*
3. **Orphan cleanup, gated on first-load-after-watch only:** anything still
   `NotInstalled` (metadata present, jar gone) has its `.pw.toml` deleted, **with no
   trash**. Watcher-triggered rescans do **not** clean orphans.

**No parse results are cached across runs.** Every jar is re-opened and re-scanned on
every model construction — no hash check, nothing persisted. In-session only,
`applyUpdates` skips re-parse when `lastModified` is unchanged. `[prism]`

For Nebula: a tiny on-disk parse cache keyed on `(path, mtime, size)` is a few lines and
removes a full jar rescan per launch.

## C.13 (7) Remote icon caching, and the webp story

Two premise corrections: **`Net::ApiDownload` does not exist** (it is `Net::ApiRequest`),
and **`ModIconResolver` does not exist anywhere in the tree.**

### The fetch flow `[prism]` https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/ui/pages/modplatform/ResourceModel.cpp

1. `data(Qt::DecorationRole)` calls `getIcon()` on **every repaint of every visible
   row**, painting a placeholder when it returns nothing.
2. Probe `QPixmapCache` keyed on the **full icon URL string**.
3. **In-flight dedupe** via a `QSet<QUrl>` — yes, deduped.
4. **Failure blacklist** via another `QSet<QUrl>`, **never cleared by `clearData()` or
   `refresh()`** — a failed icon is never retried until the dialog closes.
5. Cache path = bucket + **`"logos/%1"`** where `%1` is the **lowercase hex SHA-1 of the
   percent-encoded URL — 40 hex chars, no file extension.**

### On-disk layout

| Surface | Path |
| --- | --- |
| Modrinth **mod** icon | `<dataPath>/cache/ModrinthPacks/logos/<40-hex-sha1>` — **no extension** |
| CurseForge **mod** icon | `<dataPath>/cache/FlameMods/logos/<40-hex-sha1>` — **no extension** |
| Description images | `<dataPath>/cache/<bucket>/images/<40-hex-sha1>` |
| Modrinth **modpack** icon | `<dataPath>/cache/ModrinthModpacks/logos/<slug>.<ext>` — **has extension** |

The mod browser and modpack browser use **two different filename schemes**, and only the
modpack one gives Qt an extension hint. Index is a JSON file at `<dataPath>/metacache`,
entries carrying `base`, `path`, `md5sum`, `etag`, `last_changed_timestamp` and
`eternal` or `current_age`/`max_age`; **default expiry one week**, conditional requests
via `If-Modified-Since` / `If-None-Match`. `[prism]`
https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/Application.cpp

### WebP — it was a packaging failure, not a Qt capability gap

**Prism has zero webp-specific code on the project-icon path.** The mechanism is
implicit: raw bytes → extensionless file (`MetaCacheSink` **never inspects
`Content-Type`**) → `QIcon(path)` → Qt content-sniffs across registered plugins.

**And it broke.** Issue [#4685](https://github.com/PrismLauncher/PrismLauncher/issues/4685)
— *"Some Description icons are BLANK"* on Windows / Prism 10.0.x / Qt 6.10.1: Modrinth
icons blank across mod, modpack, resourcepack and shaderpack browsers, absent in 9.4,
reproduced on three machines. Duplicate:
[#4915](https://github.com/PrismLauncher/PrismLauncher/issues/4915) (~99% of Modrinth
icons blank; clearing the cache did nothing). `[issue]`

The fix — PR [#4739](https://github.com/PrismLauncher/PrismLauncher/pull/4739), merged
2026-01-18, *"[Windows] Fix JPEG and WEBP in MinGW builds"* — **touches only
`launcher/CMakeLists.txt`**, globbing `imageformats/*.dll` and installing their
`GET_RUNTIME_DEPENDENCIES`. `[issue]`

> **The correct framing: the `qwebp`/`qjpeg` plugins were shipped; their transitive
> dependency DLLs (libwebp, libjpeg) were not, so the plugins failed to load and
> `QImageReader` silently returned nothing — blank icons, no error, no log.**

**This is precisely the class of bug Nebula avoids by decoding with the `image` crate
statically linked (D.2)** rather than relying on a dynamically-discovered plugin. There
is no plugin to fail to load.

Also: a decode failure is **not treated as a failure** by Prism — the *network* task
succeeded, so the URL enters neither the in-flight set nor the blacklist, and the next
repaint re-enters the fetch path. Related: issues
[#5364](https://github.com/PrismLauncher/PrismLauncher/issues/5364) and
[#5384](https://github.com/PrismLauncher/PrismLauncher/issues/5384) — feeding a webp to
the skin path **freezes the UI** instead of rejecting it. `[issue]`

## C.14 (9) Known pitfalls, from Prism's issue tracker

All `[issue]`.

**CurseForge blocked mods.** `allowModDistribution` appears **literally zero times in the
Prism repo** — Prism never names the API field.
* [#599](https://github.com/PrismLauncher/PrismLauncher/issues/599) — **open since Dec
  2022.** Two failure modes: the version dropdown says *"No valid version found!"*, **or**
  a version is selectable, the download reports success, and the mod **silently vanishes
  on refresh.**
* [#3220](https://github.com/PrismLauncher/PrismLauncher/issues/3220) (open) — the
  blocked-mods dialog exists **only on the modpack import path**; downloading an
  individual blocked mod just errors with no link.
* [#4099](https://github.com/PrismLauncher/PrismLauncher/issues/4099) (open) — the dialog
  shows only the first blocked mod, the rest below an easily-missed scroll; users launch
  with mods missing.
* [#3218](https://github.com/PrismLauncher/PrismLauncher/issues/3218) (open) — CF mods
  whose metadata lacks modloader info fail to download at all.
* Prism has **deliberately declined** URL reconstruction: PR
  [#5275](https://github.com/PrismLauncher/PrismLauncher/pull/5275) closed unmerged,
  [#394](https://github.com/PrismLauncher/PrismLauncher/issues/394) won't-fix. This
  corroborates B.5.

**`+` and spaces in filenames — two distinct mechanisms, do not conflate them.**
* [#635](https://github.com/PrismLauncher/PrismLauncher/issues/635) (closed) —
  manually-downloaded files with `+` in the name were **never detected by the
  blocked-mods watcher** while siblings were, **despite verified matching SHA-1s.** This
  is a filename *matching* hazard; no URL-encoding root cause was proven in the thread.
* [#1226](https://github.com/PrismLauncher/PrismLauncher/issues/1226) (closed) — a
  genuine **partial**-encoding bug: Prism emitted `modelfix-1.8-fabric %281%29.jar` where
  it should have emitted `modelfix-1.8-fabric%20%281%29.jar` — **parentheses
  percent-encoded, space not.**

The lesson matches A.4: **never re-encode a provider-supplied URL, and never derive the
disk filename from the URL.** Use `url` verbatim for the request and `filename` verbatim
for disk.

**Forge + NeoForge dual-declaration — UNVERIFIED, and the source explains why.** No issue
exists for a dual-toml jar being mis-detected, because (C.9) both filenames hit the same
parse branch and **Prism never derives loader identity from which toml is present.** The
failure mode cannot occur in Prism's parser as written. **If Nebula infers loader from
the toml filename, it takes on a risk Prism sidestepped by not inferring at all.** The
real loader bugs live in provider version-list *tagging*:
[#3988](https://github.com/PrismLauncher/PrismLauncher/issues/3988) (closed
`not_planned` — GeckoLib offers downgrades because later versions are tagged `forge`
only) and [#818](https://github.com/PrismLauncher/PrismLauncher/issues/818) (updating in
a Quilt instance pulled the Fabric build).

**One malformed toml crashes the whole launcher.**
[#6047](https://github.com/PrismLauncher/PrismLauncher/issues/6047) — **open**, Sept
2026, v11.0.3: a NeoForge toml with `$` in a key position throws out of `ReadMCModTOML`
and **takes down the entire app**, triggered just by opening the mods list. Fix in
flight: PR [#6061](https://github.com/PrismLauncher/PrismLauncher/pull/6061). Sibling: PR
[#6036](https://github.com/PrismLauncher/PrismLauncher/pull/6036) adds index validation to
stop a toggle crash.

> **Wrap per-jar parsing in a hard failure boundary.** In Rust: return
> `Result<ModDetails, _>` per jar and log-and-skip on error; never let one bad jar
> propagate.

**Metadata issues.** PR [#1543](https://github.com/PrismLauncher/PrismLauncher/pull/1543)
(invalid TOML written; one corrupt file aborted the whole load);
PR [#2520](https://github.com/PrismLauncher/PrismLauncher/pull/2520) *"completly remove
metadata on refresh"* (leftover metadata folders survived deletion);
[#5262](https://github.com/PrismLauncher/PrismLauncher/issues/5262) (blank `side` in every
index file); [#1479](https://github.com/PrismLauncher/PrismLauncher/issues/1479) (mods
lost provider attribution after the 7.0 update).

---

# D. Rust crates

Verified against the actual worktree `H:\nebularust` on
2026-09-11. MSRV pin is **1.97.0** (`rust-toolchain.toml`), workspace `rust-version =
"1.97"`, `edition = "2024"`, `resolver = "3"`. `[repo]` `Cargo.toml`,
`rust-toolchain.toml`

License gate `[repo]` `deny.toml`: allowed = `Apache-2.0`, `BSD-2-Clause`,
`BSD-3-Clause`, `BSL-1.0`, **`GPL-3.0-only`**, `ISC`, `MIT`, `MIT-0`, `Unicode-3.0`,
`Zlib`. `exceptions = []`.

## D.1 Already in the tree — reuse these, add nothing

| Crate | Version | Where | Use for |
| --- | --- | --- | --- |
| `reqwest` | 0.13.5 (`blocking`, `json`, `native-tls`) | nebula-core | all HTTP |
| `serde` / `serde_json` | **1.0.229** / 1.0.151 (resolved) | workspace | API models. `[corrected 2026-09-11: 2 refuters, `Cargo.lock:4740-4741`]` `1.0.228` is only the workspace *manifest floor* (`Cargo.toml` `serde = { version = "1.0.228" }`); every other row in this table quotes the **resolved lock** version, so the old value was inconsistent with its neighbours. |
| `url` | 2.5.8 | workspace | **query encoding — see D.4** |
| `toml` | 1.1.5 | nebula-core | packwiz `.pw.toml` read/write |
| `zip` | 8.6.0 (`deflate-flate2-zlib-rs`) | nebula-core | jar parsing (`fabric.mod.json`, `mods.toml`, icons) |
| `sha1` | 0.11.0 | nebula-core | Modrinth sha1 hashes |
| `sha2` | 0.11.0 | nebula-core | **sha512** for Modrinth (`Sha512`), sha256 already used |
| `md-5` | 0.11.0 | nebula-core | CurseForge `hashes[].algo == 2` |
| `hex` | 0.4.3 | nebula-core | hex encoding of digests |
| `globset` | 0.4.20 | nebula-core | file matching |

`[repo]` `crates/nebula-core/Cargo.toml`

`crates/nebula-core/src/hash.rs` already streams md5 + sha1 + sha256 in one pass over a
file (`Digests { size, md5, sha1, sha256 }`). `[repo]` — extend that struct with
`sha512` rather than writing a second hasher; the file is read once.

## D.2 `image` — already compiled, but **NOT with webp at runtime**

`image v0.25.10` is already in `Cargo.lock` and already compiled, pulled in twice by
Slint 1.17.1. `[repo]` `cargo tree -p nebula -i image`:

```
image v0.25.10
└── i-slint-compiler v1.17.1      (build/proc-macro side)
image v0.25.10
└── i-slint-core v1.17.1          (runtime side)
```

**Critical finding.** `resolver = "3"` does not unify features between the
build-dependency graph and the normal graph, and the two instances have *different*
feature sets. `[repo]` `cargo tree -e features --edges normal -i image`:

| Instance | `image` features enabled |
| --- | --- |
| via `i-slint-compiler` (build/proc-macro) | full `default` → `avif, bmp, dds, exr, ff, gif, hdr, ico, jpeg, png, pnm, qoi, rayon, tga, tiff, **webp**` |
| via `i-slint-core` (**runtime**) | **`jpeg` and `png` only** |

So **Slint's runtime image loading decodes PNG and JPEG, not WebP.** Modrinth serves
`.webp` icons (A.6). Handing a `.webp` file to Slint will fail.

Slint's own documentation confirms this independently `[docs]`
https://docs.rs/slint/1.17.1/slint/struct.Image.html — on `Image::load_from_path`:

> "Supported formats are SVG, PNG and JPEG. Enable support for additional formats
> supported by the image crate (AVIF, BMP, DDS, Farbfeld, GIF, HDR, ICO, JPEG, EXR,
> PNG, PNM, QOI, TGA, TIFF, WebP) by enabling the `image-default-formats` cargo
> feature."

`image-webp v0.2.4` *is* already in the lockfile `[repo]` `Cargo.lock:2624`, reachable
in the normal graph via `resvg v0.47.0` (for webp inside SVGs) and in the build graph
via the compiler-side `image`.

### Two ways to fix it — take the second

**Option 1 (rejected): add `"image-default-formats"` to slint's feature list.**
Documented, zero code. But measured `[repo]`
`cargo tree -p nebula --features slint/image-default-formats --edges normal -i rav1e`:
this **drags `ravif v0.13.0` + `rav1e v0.8.1` out of the host/proc-macro graph and into
the target link graph.** Today they only reach `i-slint-compiler` (host build); with
that feature they reach `i-slint-core` and get compiled for the target. rav1e is one of
the slowest crates in the ecosystem to compile. Paying an AV1 encoder to display a 96px
mod icon is absurd.

**Option 2 (recommended): a narrow direct dep, decode ourselves.** In
`crates/nebula/Cargo.toml`:

```toml
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
```

* **No new crate versions enter `Cargo.lock`** — `image` 0.25.10 and `image-webp` 0.2.4
  are already there. No new licences, no `deny.toml` churn, no rav1e.
* Decode to RGBA8, then `slint::Image::from_rgba8(SharedPixelBuffer<Rgba8Pixel>)`
  `[docs]` https://docs.rs/slint/1.17.1/slint/struct.Image.html. This path does not
  depend on Slint's internal feature gates at all.
* `image::guess_format(&bytes)` sniffs magic bytes — use it instead of trusting the URL
  extension (A.6).

`image` is MIT OR Apache-2.0; `image-webp` is MIT OR Apache-2.0 — both inside the
`deny.toml` allow list.

## D.3 MurmurHash2 for CurseForge fingerprints

`cargo info murmur2` `[live]`:

```
murmur2 v0.1.0
license: Apache-2.0
repository: https://github.com/jcaesar/murmur2rs
```

Public surface, per https://docs.rs/murmur2/0.1.0/murmur2/ `[docs]`:
`KAFKA_SEED`, `murmur2()`, `murmur2ne()`, `murmur2a()`, `murmur2ane()`, `murmur64a()`,
`murmur64ane()`, `murmur64b()`, `murmur64bne()`.

**The crate has no CurseForge variant and no CurseForge seed constant** — only Kafka is
mentioned. `[docs]` The CurseForge-specific part (byte filtering) would be our code
regardless.

**Recommendation: implement MurmurHash2 inline, ~20 lines, do not add the dep.**

* The algorithm is 20 lines; a 0.1.0, single-author, single-release crate is a worse
  trade than that.
* The CurseForge variant's whole difference is *what you feed it* (whitespace-filtered
  bytes, seed 1, and the **filtered** length as the length term). That filtering is ours
  either way, so the dep saves almost nothing.
* Constants: `m = 0x5bd1e995`, `r = 24`, seed `1`, unsigned 32-bit, **filtered length as
  the length term**. Full spec and Prism's implementation in **B.6**.
* **The algorithm is verified, not assumed** — B.6 records a known-answer test that
  reproduced a live `fileFingerprint` exactly (`2298786945` for
  `jei-1.20.1-forge-15.59.0.210.jar`). Port that pair straight into a `#[test]`; it is
  the only thing standing between a correct implementation and every fingerprint lookup
  silently returning "unknown mod".

Prism's own `libraries/murmur2/` is **public domain**, not GPL (B.6), so its constants
are safe to reference directly — but the whole thing is ~20 lines and the reference
implementation is Austin Appleby's public-domain MurmurHash2 anyway.

## D.4 Query encoding — `url`, already present; skip `urlencoding`

`cargo info urlencoding` `[live]`: `urlencoding v2.1.3`, MIT,
https://crates.io/crates/urlencoding/2.1.3 — fine licence, but **unnecessary**.

`url v2.5.8` is already a workspace dependency `[repo]` `Cargo.toml`. Use
`Url::query_pairs_mut().append_pair(...)`, which percent-encodes values correctly,
including the JSON `facets` / `loaders` / `ids` arrays. `reqwest` also accepts
`.query(&[("facets", json_string)])` and encodes for you.

**Do not hand-encode.** The Modrinth facets/loaders params are JSON strings containing
`[`, `]`, `"`, `:` and `,`, all of which must be percent-encoded.

**And do not encode `files[].url`** — it arrives already encoded (A.4). Feed it to
`reqwest` as a string, or parse with `Url::parse` (which is idempotent on an
already-encoded URL) — but never run it through `query_pairs_mut` or `urlencoding::encode`.

## D.5 `toml` for packwiz

`toml 1.1.5` is already present `[repo]`. It round-trips through `serde`, so the
`.pw.toml` shape in section C maps to a plain `#[derive(Serialize, Deserialize)]`
struct. Note packwiz uses **kebab-case keys** (`hash-format`, `mod-id`, `file-id`,
`project-id`) — use `#[serde(rename_all = "kebab-case")]`.

## D.6 What this repo already has for CurseForge

`crates/nebula-core/src/curseforge.rs` `[repo]` already contains, for the modpack-import
path:

* `pub const API_BASE: &str = "https://api.curseforge.com/v1";`
* `pub const API_KEY: &str = "$2a$10$JL4kTO/N/oXIM6o3uTYC3eLxGrOI4BIAqpX4vAFeIPoXiTtagidkK";`
  — described in-source as *"Upstream Nebula's key, kept so imports keep working out of
  the box."*
* the header is sent as `("X-API-KEY", API_KEY)` via `remote::get_json_with_headers`
  (line 210)
* `downloadUrl` is already modelled as `Option<String>` (line 222-223), and when it is
  `None` the code pushes a `ManualDownload` pointing at
  `https://www.curseforge.com/minecraft/mc-mods/{slug}/download/{fileId}` (**lines 179-181**;
  line 177 is the `counters.manual.push(ManualDownload {` opener `[corrected 2026-09-11: repo read]`)

**The CurseForge key policy question is therefore already answered de facto in this
repo** — a key is shipped in the binary today. The mod browser should reuse the same
constant and the same manual-download fallback rather than inventing a second path.
Whether shipping it is *permitted* is in section B.

---

# Decisions the planner must make

Each of these is a real fork with evidence on both sides. None is decided here.

### D1. Metadata store: packwiz `.pw.toml` per mod, or one JSON index?

| | packwiz `.pw.toml` (Prism-compatible) | single `mods/index.json` |
| --- | --- | --- |
| Interop | Prism, packwiz, ATLauncher, MultiMC forks can all read it | none |
| Cost | one file per mod, kebab-case keys, `toml` crate already present (D.5) | one file, `serde_json` already present |
| Lookup by project id | **O(n): parse every file until one matches** (C.0b, `MetadataHandler`) | O(1) |
| Corruption blast radius | one mod (after PR #1543) | **the whole index** |
| Prism's own extensions | must emit `x-prismlauncher-*` (C.0b) or lose loaders/MC-versions/release-type | n/a |
| Concurrent writes | per-file, naturally safe | needs whole-file rewrite |

The real question: **is Prism interop a goal, or just a nice story?** Nebula provisions
servers; the mods folder it produces is consumed by a server, not by Prism. If interop is
not a real requirement, `.pw.toml` is paying O(n) lookups and a five-key non-standard
extension set for nothing. If it is, emit `.pw.toml` **and** the `x-prismlauncher-*` keys,
because without them a Prism user loses the loader/version columns entirely.

Middle option worth costing: write `.pw.toml` as the source of truth **plus** a derived
`mods/.index/_index.json` lookup cache keyed by `(provider, project_id)` and by hash,
rebuilt when any `.pw.toml` mtime changes.

### D2. CurseForge API key policy

Four options, in increasing order of legal comfort and decreasing order of convenience:

1. **Keep shipping the inherited upstream key** (status quo — `[repo]`
   `crates/nebula-core/src/curseforge.rs:20`). Works today. **Contradicts the 3rd Party
   API Terms clause that the key "may not be shared with any third party"** (B.1). Also
   inherits whatever quota/abuse history that key carries.
2. **Ship our own key + user override**, Prism-style, and put the ToS-acceptance notice
   in the README. Same legal posture as Prism, which is at least a well-trodden one.
3. **Ship no key; CurseForge features are off until the user pastes one.** Fully
   compliant, and matches Prism's own `SupportsFlame` capability gate — the code path
   already exists in Prism to hide every CF surface when the key is empty.
4. **Modrinth only.** Modrinth needs no key, has a documented rate limit, documented
   headers, and no distribution opt-out (B.5). CurseForge is the entire source of the
   blocked-mod problem, the missing rate-limit headers, and the ToS problem.

Note this repo **already** ships a key for the modpack-import path, so option 3/4 for the
browser while option 1 persists for import is an inconsistent posture that should be
settled once.

### D3. Version selection default

Prism installs **the newest file regardless of release type** (C.2) — verified to matter:
CurseForge's newest 1.20.1 Forge JEI is a **beta** (`releaseType: 2`, `fileDate 2026-09-10`).
`[corrected 2026-09-11: 3 refuters — the companion claim "Modrinth's newest 1.20.1 Sodium is a beta"
was **false**; Sodium's newest 1.20.1 version is the `0.5.13` release. One verified example is
enough to make the decision; two would have been nicer.]`

* **Copy Prism** — consistent with what users expect from a launcher.
* **Diverge: newest `release`, falling back to newest-any when no release matches** —
  correct for a server-provisioning tool, and only a few lines.

Recommend diverging. Decide explicitly.

### D4. Forge/NeoForge widening on 1.20.1

Modrinth does not alias them (A.7); Prism widens client-side, one-directionally, on exact
string `"1.20.1"` (C.4). Options: copy exactly; skip the widening (fewer surprises, more
"where is my mod?"); or widen and **label** widened results in the UI.

### D5. Icon handling

* **Format:** Modrinth serves `.webp` *and* `.png` from the same field (A.6); CurseForge
  serves `.jpeg`/`.png` (B.7). Decode with the `image` crate and sniff magic bytes
  (D.2) — do **not** trust extensions, and do not rely on a runtime-loaded plugin (C.13).
* **Cache key:** Prism uses `sha1(percent-encoded URL)` with **no extension** (C.13).
  Storing the sniffed extension costs nothing and makes the cache dir debuggable.
* **Cache location:** needs a decision — alongside the instance, or in a global
  `%LOCALAPPDATA%` cache?
* **Note the ToS tension:** the CF 3rd Party Terms literally forbid caching data obtained
  through the API (B.1). See Risks.

### D6. Dependency resolution scope

Prism follows **only `REQUIRED`**, silently ignores `INCOMPATIBLE`, has no cross-provider
mapping, and **silently drops unresolvable dependencies** (C.5). Decide: do we surface
optional deps? Do we *warn* on incompatible? Do we hard-fail on an unresolvable required
dep rather than dropping it silently? For a provisioning tool, silently dropping a
required dependency produces a broken server — recommend hard-failing.

### D7. Update-check batching

Modrinth's `POST /v2/version_files/update` accepts many hashes but Prism sends **one
loader per request** (C.6). With a single-loader instance that is one request for the
whole mods folder. Decide whether to send the union of loaders in one call (fewer
requests, but the response is then ambiguous about which loader matched).

### D8. Where does this code live?

`nebula-core` already owns `curseforge.rs`, `hash.rs`, `remote.rs` and the `zip`
dependency. The browser is a `nebula` (GUI) concern but the API clients, hashing, jar
parsing and metadata are all core. Recommend: everything except the Slint views goes in
`nebula-core`, reusing `remote.rs` and extending `hash.rs` with sha512 + murmur2.

---

# Risks

**R1 — CurseForge ToS vs. everything we want to build.** The 3rd Party API Terms forbid
sharing the API key with any third party *and* forbid caching any data obtained through
the API (B.1, quoted verbatim). A shipped open-source launcher with an icon and metadata
cache violates a literal reading of both. Prism does it anyway and pushes acceptance
downstream via its README. **This is a decision for a human, not for the implementation.**

**R2 — CDN key enforcement lands and downloads start 401ing.** CurseForge announced
`x-api-key` would become *required* on `edge.forgecdn.net` in mid-July; as of 2026-09-11
it still is not enforced `[live]` (B.1). When it flips, unauthenticated downloads break
with `401 {"error":"Unauthorized",...}`. *Mitigation: send `x-api-key` on CDN requests
from day one, and follow the `edge.` → `mediafilez.` redirect with the header intact —
the exact place Prism's host-matched proxy would drop it.*

**R3 — Blocked mods are 2% of the popular set and there is no clean fix.** Verified live:
5 of the top 250 1.20.1 mods have `allowModDistribution: false`, including EntityCulling
and 3D Skin Layers (B.5). `download-url` returns 403. Prism has fought this since 2022
and issue #599 is still open. *Mitigation: implement Prism's Modrinth-by-sha1 fallback
(B.5) first, manual-download link second, and **never** silently succeed — Prism's worst
failure mode is a download that reports success and then the mod vanishes.* **Note this is a
deliberate superset of Prism, not a copy** `[corrected 2026-09-11: 2 refuters]`**: Prism only runs that
fallback on the modpack-import path (`FileResolvingTask`, B.5); its mod browser simply hides
blocked versions and its update check only shows a website link. Nebula applies it on browse,
install and update.*

**R4 — Reconstructing the CDN URL is technically possible and must be explicitly
forbidden.** Verified: the reconstructed URL 302s and serves the bytes even for a blocked
file (B.5). It will look like an obvious fix to a future contributor. *Mitigation: a
comment in the code and a line in CONTRIBUTING saying Prism rejected this deliberately
(PR #5275 closed unmerged).*

**R5 — Modrinth v2 will eventually return HTTP 410.** The docs state v2 will be
deprecated and eventually return permanent 410 once v3 is GA (A.1). The live v2 endpoint
is already returning v3-shaped fields (`environment`, `all_project_types`,
`disclosure_types`) while the docs mark `client_side`/`server_side` deprecated. *Mitigation:
one `API_V` constant, every field `Option<_>`, and never fail a deserialize on an unknown
field.*

**R6 — The `+`-in-filename trap, in both directions.** Modrinth's `url` and `filename`
differ (`%2B` vs `+`) for a very popular mod (A.4). Re-encoding the URL 404s;
deriving the filename from the URL writes a file literally named `...%2B...`. **"Prism has
shipped bugs on both sides" was an overstatement** `[corrected 2026-09-11: live read of
`ResourceDownloadTask.cpp` L72-73 and both issues]`: Prism's Modrinth download path is
**correct** (downloads `m_pack_version.downloadUrl` verbatim, saves `getFilename()` ==
`m_pack_version.fileName`). #635 is the CurseForge blocked-mod *watcher* failing to match
manually-downloaded `+` filenames (no URL-encoding root cause proven), and #1226 is the
**mrpack export** path emitting `modelfix-1.8-fabric %281%29.jar` — parentheses encoded, space
not. C.14's framing is the accurate one. The trap is still real for us; the precedent is
narrower than claimed. *Mitigation: a unit test on the live Sodium URL/filename pair, plus one
on a legacy `%2B`-in-`filename` row (A.4). Never call `urlencoding::encode` on a provider URL
(D.4).*

**R7 — No CurseForge rate-limit headers means no adaptive backoff.** Verified live: zero
`X-Ratelimit-*` headers, no documented number, and a contractual quota that is
*"to be decided… upon Overwolf's sole discretion"* (B.1). We can be cut off with no
warning signal. *Mitigation: a conservative fixed client-side throttle plus request
coalescing; prefer `latestFilesIndexes` from the search response over per-mod `/files`
calls.*

**R8 — Enabling `slint/image-default-formats` would drag `rav1e` into the target build.**
Measured (D.2). It is the obvious-looking fix for webp and it is the wrong one.
*Mitigation: the narrow `image` dep is already the recommendation.* **The `deny.toml`
guard originally suggested here does not exist and cannot be written.** `[corrected 2026-09-11:
cargo-deny docs + a real `cargo deny check bans` run against the unmodified repo]`
cargo-deny's `[graph]` section offers only `targets`, `exclude`, `exclude-dev`,
`exclude-unpublished`, `all-features`, `no-default-features`, `features` — **there is no way
to scope a ban to the normal (non-build/proc-macro) graph**, and `bans.deny` supports only
`wrappers` (a parent-crate allow-list), which cannot discriminate because the parent is
`ravif` on **both** the host and the target path. Probed with cargo-deny 0.20.2 on the
**unmodified** repo using `deny = [{ crate = "rav1e" }]`: `bans FAILED` —
`error[banned]: crate 'rav1e = 0.8.1' is explicitly banned`, path
`rav1e ← ravif ← image ← i-slint-compiler ← slint-build (build) / slint-macros`. The guard
would fire on day one, before anyone touches the feature.

*For the same reason a plain CI grep of `Cargo.lock` for `rav1e` is also useless — **rav1e is
already in the lockfile today** via the host/proc-macro graph. The check must discriminate
graphs, and **the exact edge filter matters** `[live 2026-09-11: all three invocations run
against this worktree]`:*

```
cargo tree -p nebula -e normal            -i rav1e   # NOT empty (reaches it through slint-macros, a proc-macro)
cargo tree -p nebula -e no-proc-macro     -i rav1e   # NOT empty (reaches it through slint-build, a build-dependency)
cargo tree -p nebula -e normal,no-proc-macro -i rav1e  # "warning: nothing to print."  <-- this is the guard
```

*With `--features slint/image-default-formats` that last one **does** print a tree rooted at
`i-slint-core`. So the CI step is: run*
`cargo tree -p nebula -e normal,no-proc-macro -i rav1e` *and fail if it emits a dependency
tree. **Assert on the output, not the exit code** — cargo exits 0 and prints
`warning: nothing to print.` in the passing case. A comment on the slint feature list is the
cheap half of the same guard.*

**R9 — One malformed jar taking down the process.** Prism has an **open** crash
(#6047) where a NeoForge toml with `$` in a key position kills the whole launcher on
opening the mods list (C.14). *Mitigation: per-jar `Result`, log-and-skip; and a fuzz-ish
test over a few deliberately corrupt jars.*

**R10 — `deny.toml` allows `GPL-3.0-only` while the workspace ships MIT.** `[repo]`
`deny.toml`. Given that the entire premise of this document is *"Prism is GPL, do not
copy it"*, the licence gate will **not** catch an accidental GPL dependency. Separately,
`OFL-1.1` is **not** in the allow list, so a bundled font would need an `exceptions`
entry. *Mitigation: drop `GPL-3.0-only` from `allow` unless something in the tree
genuinely needs it, and add OFL only when a font actually arrives.*

**R11 — `murmur2` implementation drift.** The algorithm is verified (B.6) but it is easy
to break silently: use the file length instead of the filtered length, forget the seed,
or use signed arithmetic, and every fingerprint lookup returns "unknown mod" with no
error. *Mitigation: the known-answer test from B.6 is mandatory, not optional.*

**R12 — Prism's `isPreferred` clobbering bug is easy to reproduce accidentally.** (C.3)
A version whose loader-specific file is not flagged `primary` resolves for download but
is never offered as an update. *Mitigation: keep "which file to download" and "is this
version an update" as two independent computations.*

---

# E. Gaps found by the completeness critic

Read as an implementer of both providers, dependency resolution, update checks, jar
metadata/icons and the JSON index (D1), this document was **complete on the traps and thin
on the boring parts you cannot ship without**: exact error bodies, enum tails, batch
semantics, and the shape of the endpoints the earlier sections only name.

Everything marked `[live 2026-09-11]` below was resolved on the spot against the real API
(Modrinth with `User-Agent: nebula-qa/0.1`, CurseForge with the key at
`crates/nebula-core/src/curseforge.rs:20`) or against the served OpenAPI documents. Items
marked **OPEN** are genuinely unresolved and are the ones to chase before coding the
matching module.

## E.1 Modrinth

### E.1.1 Error bodies — RESOLVED `[live 2026-09-11]`

Nowhere in §A is there a single non-200 body. There are exactly three shapes:

| Situation | Status | Body |
| --- | --- | --- |
| Unknown project (`/v2/project/zzzz-nonexistent-zzzz`) | **404** | **empty** — zero bytes, no JSON |
| Unknown hash (`GET /v2/version_file/<40 zeros>`) | **404** | **empty** |
| Malformed query JSON (`/v2/search?facets=notjson`) | **400** | `{"error":"request_error","description":"deserializing JSON data","details":["expected ident at line 1 column 2"]}` |
| Rate limited | **429** | `{"error":"ratelimit_error","description":"rate limit exceeded; retry after N milliseconds"}` (A.1) |
| Bad scope / auth | **401** | `AuthError` `{error, description}` `[docs]` |

**Why it matters:** an empty 404 body will panic a `serde_json::from_str` that assumes an
error envelope, and `details` is **not** in the spec's `InvalidInputError` schema
(`{error, description}` only) — so the error struct needs
`#[serde(default)] details: Vec<String>` plus a "body was empty" branch keyed on the status
code, not on the body.
**Where to look:** https://docs.modrinth.com/openapi.yaml
(`components.schemas.InvalidInputError`, `AuthError`).

### E.1.2 The single-version endpoint was missing from the A.5 table — RESOLVED (row added)

C.5 says dependency resolution does `GET /v2/version/{id}` when a `version_id` is pinned,
but that endpoint appears in **no** table in §A. Its sibling `GET /v2/versions?ids=[...]`
*is* listed. **The row has been added to A.5 in this pass**; the response is a bare version
object (the same shape as an entry of `/v2/project/{id}/version`).
**Where to look:** https://docs.modrinth.com/api/operations/getversion/

### E.1.3 Batch endpoints silently drop unknown ids — RESOLVED `[live 2026-09-11]`

`GET /v2/projects?ids=["AANobbMI","ZZZZZZZZ"]` returns **HTTP 200 with a one-element array**
— no error, no null placeholder. Combined with A.3's warning that the array is not keyed and
not order-preserving, this means **a caller cannot tell "not found" from "reordered" without
matching on `id` and diffing against the request set.**
**Why it matters:** the update/reconcile path batches project lookups for the whole mods
folder; a silently short array becomes "this mod has no project page" in the UI.

### E.1.4 `project_type` values are not enumerated — RESOLVED `[live 2026-09-11]`

`GET https://api.modrinth.com/v2/tag/project_type` →
`["mod","modpack","resourcepack","shader","plugin","datapack","minecraft_java_server"]`.

**Mods-only filtering (the scope line in the plan) needs two things, not one:** the facet
`[["project_type:mod"]]` **and** a check on the hit's own `project_type` field, because the
v2 facet key is rewritten to the plural `project_types` (A.2) and therefore matches any
project that has *ever* published a mod-typed version. `resourcepack`, `shader` and
`datapack` are excluded by the same facet; there is no separate "exclude" syntax.

### E.1.5 Projects that are both a mod and a modpack — RESOLVED `[live 2026-09-11]`

They exist and are rare: `facets=[["project_type:mod"],["all_project_types:modpack"]]` →
**3 hits** (`exodus`, `performized`, `imaginefun`), each with
`project_type: "modpack"` and `all_project_types: ["mod","modpack"]`.
**Why it matters:** installing one of those into a `mods/` folder installs a `.mrpack`, not a
jar. **Rule for Nebula: trust `project_type`, not `all_project_types`, when deciding what a
project *is*; use `all_project_types` only for search recall.** Also filter the chosen
*version*: a version's own `loaders` array contains `mrpack` for modpack versions.

### E.1.6 `dependencies[]` can name nothing at all — RESOLVED (spec read `[live 2026-09-11]`)

In `VersionDependency` the **only required property is `dependency_type`**; `version_id`,
`project_id` and `file_name` are all nullable. So a dependency entry with
`{project_id: null, version_id: null, file_name: "somelib-1.0.jar", dependency_type:
"embedded"}` is schema-legal and common for embedded jars.
**Why it matters:** D6 resolves `required` recursively — a `required` entry with both ids
null is unresolvable *by construction*, and under D6's "hard-fail on unresolvable required"
rule it would block an otherwise fine install. **Treat "required with no ids" as a warning,
not a failure**; only a required dep with an id that fails to resolve is a hard error.
**Where to look:** https://docs.modrinth.com/openapi.yaml `components.schemas.VersionDependency`.

### E.1.7 CDN download semantics — OPEN

§A.4 gives the URL rule but nothing about the transfer: whether `cdn.modrinth.com` requires
the API `User-Agent` (it is a different host from `api.modrinth.com`), whether it honours
`Range`, what it sets for `ETag` / `Last-Modified` (relevant to the icon cache, D5), and
whether the 300/min budget applies to it. Prism sends a courtesy
`modrinth-download-meta` header on CDN downloads (C.1) and nothing else.
**Where to look:** https://docs.modrinth.com/api/ (Ratelimits section) — and one `curl -D -`
against a real `files[].url`.

## E.2 CurseForge

### E.2.1 `GET /v1/mods/{modId}/files/{fileId}/download-info` — RESOLVED `[live 2026-09-11]`, and it is the better blocked-file probe

The endpoint was undocumented in §B until this pass. It returns
`ApiResponseOfListOfDownloadInfo`, i.e. `{"data": [DownloadInfo]}`:

```jsonc
// GET /v1/mods/238222/files/8851009/download-info  -> 200
{"data":[{"type":1,
          "downloadUrl":"https://edge.forgecdn.net/files/8851/9/jei-1.20.1-forge-15.59.0.210.jar",
          "fileName":"jei-1.20.1-forge-15.59.0.210.jar",
          "fileLength":1825651,"fileSizeOnDisk":4350698,
          "hashes":[{"value":"3bbb…db3","algo":1},{"value":"bdfe…492","algo":2}]}]}
```

**For a blocked file it returns HTTP 200 with `downloadUrl: ""` — not the 403 that
`/download-url` gives (B.5) — and it still hands you the sha1:**

```jsonc
// GET /v1/mods/448233/files/8287086/download-info  -> 200
{"data":[{"type":1,"downloadUrl":"",
          "fileName":"entityculling-forge-1.10.5-mc1.20.1.jar",
          "fileLength":1590221,"fileSizeOnDisk":1851303,
          "hashes":[{"value":"b818f6d792b79f4ec116c89704e70292bd6f72b5","algo":1},
                    {"value":"f54a5baa0a9613d13ad0e781d245338b","algo":2}]}]}
```

**Why it matters:** this is a one-call source of `(sha1, fileName, fileLength)` for the
Modrinth-by-sha1 fallback (R3) that works *on blocked files*, without pulling a `/files`
page. It also settles the `""`-vs-`null` question: `downloadUrl` is `null` on the `File`
object and `""` here — **treat `null`, `""` and absent identically**, exactly as B.4 already
advises. Schema also declares an `extraJsonData: string` field and a `DownloadInfoType`
discriminator (`type: 1` observed) that are undocumented in the rendered docs.
**Where to look:** `https://api.curseforge.com/swagger/v1/swagger.json`
(`components.schemas.DownloadInfo`, `ApiResponseOfListOfDownloadInfo`).

### E.2.2 Enum tails never enumerated — RESOLVED `[live 2026-09-11]` (served swagger, `x-enumNames`)

§B enumerates `fileStatus` and `modLoaderType` but not these, all of which appear in the
live search-hit / file JSON:

| Schema | Values |
| --- | --- |
| `ProjectFileReleaseType` | **1 Release, 2 Beta, 3 Alpha** — only three; the `releaseTypes` query param on `/files` takes this |
| `HashAlgo` | **1 Sha1, 2 Md5** — confirmed exhaustive; **there is no sha512 and no murmur2 in `hashes[]`** |
| `ModStatus` (the hit's `status` field) | 1 New, 2 ChangesRequired, 3 UnderSoftReview, **4 Approved**, 5 Rejected, 6 ChangesMade, 7 Inactive, 8 Abandoned, 9 Deleted, 10 UnderReview — **note this is a different enum from `fileStatus` (15 values), despite both using 4 = Approved** |
| `PlatformType` (the `platformType` query param, E.2.4) | 0 Empty, 1 Windows, 2 XboxOne, 3 XboxXS, 4 Linux, 5 PS4, 6 PS5, 7 Mac, 8 IOS, 9 TVOS, 10 Android, 11 Switch, 12 WindowsServer, 13 LinuxServer |
| `CoreApiStatus` | 1 Private, 2 Public |

**Why it matters:** `ModStatus` vs `fileStatus` sharing the number 4 while meaning different
things is exactly the kind of collision a shared `enum Status` in Rust would paper over.
Keep two types.

### E.2.3 `pagination.totalCount` is capped — RESOLVED `[live 2026-09-11]`

`GET /v1/mods/search?gameId=432&classId=6&pageSize=1` →
`{"index":0,"pageSize":1,"resultCount":1,"totalCount":10000}` for a corpus far bigger than
10 000. Narrow queries report honestly (JEI: 151).
**Why it matters:** rendering "10000개 결과" is a lie, and driving a "load more" affordance
off `totalCount` walks straight into the hard `index + pageSize <= 10000` 400 (B.2). Stop
paging when a page returns fewer than `pageSize` items.

### E.2.4 Category tree shape — RESOLVED `[live 2026-09-11]`

`GET /v1/categories?gameId=432&classId=6` → **52 entries**, each:

```jsonc
{"id":434,"gameId":432,"name":"Armor, Tools, and Weapons","slug":"armor-weapons-tools",
 "url":"https://www.curseforge.com/minecraft/mc-mods/armor-weapons-tools",
 "iconUrl":"https://media.forgecdn.net/avatars/6/47/635351498790409758.png",
 "dateModified":"2014-05-08T17:44:39.057Z","classId":6,"parentCategoryId":6,
 "displayIndex":0}
```

`parentCategoryId` equals the `classId` for top-level categories, so the "tree" is two deep
at most for mods. Cache it — it changes on a scale of years.
**Why it matters:** B.3 lists the endpoint with no shape at all, and a category filter is
the one search facet CurseForge ANDs (B.2), so getting the ids right is load-bearing.

### E.2.5 Batch `POST /v1/mods` silently drops unknown ids — RESOLVED `[live 2026-09-11]`

`{"modIds":[238222,1]}` → **HTTP 200, `data` has one element.** Same hazard as E.1.3, same
mitigation: diff the returned ids against the request set.

### E.2.6 A CurseForge project has exactly one `classId` — RESOLVED, and it is a real asymmetry

Unlike Modrinth (E.1.5), a CurseForge project is a mod **or** a modpack, never both, because
`classId` is scalar. **So the mod/modpack ambiguity is a Modrinth-only problem**, and
`classId=6` is a complete filter on the CurseForge side. The corresponding CurseForge trap is
the opposite one: a *mod* project can ship a **server pack** as a separate file
(`isServerPack`, `serverPackFileId` on `File`) — those are `.zip`, not jars.
**Why it matters:** the install path must reject `isServerPack: true` files, and neither §B.4
nor the plan says so.

### E.2.7 CDN transfer semantics beyond the redirect — partially RESOLVED, partially OPEN

Resolved in B.5 this pass: the `edge.` → `mediafilez.` 302 works un-keyed today, the
key-enforcement date was **July 16 2026**, and a `Range: bytes=0-0` request returns **404**
while the plain GET returns 200. **OPEN:** whether `x-api-key` survives the `edge.` → `mediafilez.` 302 in `reqwest`.
`x-api-key` is a *custom* header, not one of the `Authorization` / `Cookie` /
`Proxy-Authorization` set that HTTP clients conventionally strip on a cross-host redirect, so
it may well be forwarded — **but this was not tested and must not be assumed either way.**
It is exactly where R2's 401 will first appear, and it is the same hop where Prism's
host-matched `ApiHeaderProxy` (which matches only `edge.forgecdn.net`) would drop it.
**Where to look:** https://docs.rs/reqwest/latest/reqwest/redirect/index.html , plus one live
test with a request-echo host — or simply attach the header manually after following the
redirect yourself (`redirect::Policy::none()` + explicit second request), which sidesteps the
question entirely and is the lazy correct answer.

## E.3 Cross-cutting

### E.3.1 The CurseForge update check cannot be driven by fingerprints — RESOLVED `[live 2026-09-11]`, and plan **D7** needs changing

Plan **D7** says "CurseForge via one `fingerprints` request". Facts **C.6** says the exact
opposite about Prism: *"No fingerprint call here"* — Prism issues one
`GET /v1/mods/{id}/files?pageSize=10000&gameVersion=<mc>` **per resource** and uses
fingerprints only for metadata *recovery*. The open question was whether
`exactMatches[].latestFiles` could serve as a cheap update source instead. **It cannot.**

`POST /v1/fingerprints {"fingerprints":[2298786945]}` (the JEI 1.20.1 **Forge** file
`jei-1.20.1-forge-15.59.0.210.jar`, `gameVersions: ["Client","1.20.1","Forge","Server"]`)
returns `exactMatches[0].latestFiles` = **11 entries, completely unfiltered by MC version,
loader or release type** `[live 2026-09-11]`:

```
3040523 jei_1.12.2-4.16.1.301.jar            releaseType 1  ["1.12.2"]
3272039 jei-1.13.2-5.0.0.31.jar              releaseType 3  ["1.13.2"]
4087656 jei-1.19.2-fabric-11.4.0.286.jar     releaseType 3  ["Fabric","1.19.2"]
4087658 jei-1.19.2-forge-11.4.0.286.jar      releaseType 3  ["1.19.2","Forge"]
5060339 jei_1.12.2-4.16.1.1004.jar           releaseType 2  ["1.12.2"]
8792636 jei-1.21.1-fabric-19.51.0.418.jar    releaseType 1  ["1.21","Client","Fabric","1.21.1","Server"]
8792637 jei-1.21.1-forge-19.51.0.418.jar     releaseType 1  ["1.21","Client","Forge","1.21.1","Server"]
8792638 jei-1.21.1-neoforge-19.51.0.418.jar  releaseType 1  [...,"NeoForge",...]
8850345 jei-1.21.1-fabric-19.54.0.427.jar    releaseType 2  [...]
8850346 jei-1.21.1-forge-19.54.0.427.jar     releaseType 2  [...]
8850347 jei-1.21.1-neoforge-19.54.0.427.jar  releaseType 2  [...]
```

**It is the project's global "latest per bucket" set, and it contains no 1.20.1 entry at all**
— even though the *matched* file is 1.20.1. So a fingerprint-driven update check on a 1.20.1
Forge server would either find nothing (correct answer, wrong reason) or, with naive
"newest wins", offer **`jei-1.21.1-forge-19.54.0.427.jar`** — a file for a different
Minecraft version.

**Consequence for the plan: D7's "CurseForge via one `fingerprints` request" is wrong as
written and must be changed.** The two workable shapes are (a) Prism's — one
`GET /v1/mods/{id}/files?gameVersion=<mc>` **per installed CurseForge mod** (N requests,
server-side filtering, correct), or (b) one fingerprint call for **identification only**
(which is what C.6 says Prism uses it for) followed by (a) for the version comparison. There
is no one-request option. Fingerprints remain the right tool for **metadata recovery** of
hand-placed jars (D7's other half is fine).

### E.3.2 Timeouts, retries and backoff have no evidence behind them — OPEN

The plan's `http.rs` row asserts *connect 30 s, 3 retries at 0.5/1/2 s, `429` honoured via
`Retry-After` / `X-Ratelimit-Reset`*. **Nothing in this facts document supports any of those
numbers**, and two of the inputs are wrong or unverified:

* **Modrinth sends no `Retry-After`.** Its 429 carries `X-Ratelimit-Reset`, documented as
  **seconds until the window resets** (A.1, `[docs]`). On an ordinary 200 with a fresh window
  it reads `0` alongside `Remaining: 300` `[live 2026-09-11]` — consistent with the
  documented semantics, but a single zero does not prove them, and either reading has the
  same consequence: **a naive `sleep(reset)` can sleep zero and hot-loop. Clamp to ≥ 1 s** and
  cap the total wait.
* **CurseForge sends nothing at all** — no `Retry-After`, no `X-Ratelimit-*`, and its swagger
  declares no 429 (B.1). The retry budget there is a pure guess, which is why R7 exists.
* Whether a 5xx from either provider is safe to retry on a **POST** (`/version_files`,
  `/fingerprints`) is unstated. Both are read-only in effect, so yes — but say so.

### E.3.3 The `modindex.json` schema is only in the plan — OPEN

D1 decides the format; §D of this document says nothing about it. Missing: the version field's
migration rule, whether hashes are stored lowercase (they must be — A.5), what happens when
two namespaces hold the same filename, and whether `dependencies` records project ids or
version ids. **The facts document should carry the canonical example** so the plan and the
code cannot drift.

### E.3.4 Jar metadata is sourced entirely from Prism's parser — OPEN

C.9 is an excellent description of *what Prism reads*, and this document is explicit that
Nebula should not copy the byte-order rule. But there is **no citation to the upstream
specifications**, so "what a field legally contains" is unknown: `fabric.mod.json`'s
`schemaVersion 2`, NeoForge's `neoforge.mods.toml` required keys (`modLoader`,
`loaderVersion`, `license` are mandatory there and Prism never reads two of them), and
whether `logoFile` may be an absolute path or contain `..`.
**Why it matters:** the icon extractor opens `logoFile` **from inside the zip** — a
`../../../etc/passwd` value is a zip-slip vector if the path is ever joined to a filesystem
root, and `zip 8.6.0` will happily return an entry named that.
**Where to look:** https://fabricmc.net/wiki/documentation:fabric_mod_json_spec ,
https://docs.neoforged.net/docs/gettingstarted/modfiles/ , and the `mods.toml` schema at
https://docs.minecraftforge.net/en/latest/gettingstarted/modfiles/

### E.3.5 Icons: no size, no content-type, no failure policy — OPEN

A.6 and B.7 cover formats. Not covered: what `icon_url` returns for a project with no icon on
**CurseForge** (Modrinth returns `null`; CurseForge's `logo` object may be absent entirely),
what the largest icon a provider will serve is (relevant to the 72×72 resize in the plan),
and whether the icon hosts set `Cache-Control` / `ETag` (relevant to D5's on-disk cache).
Prism's answer to the last one is `metacache` with a **one-week default expiry** and
`If-Modified-Since` / `If-None-Match` (C.13) — that is a reasonable default to copy, but it
is Prism's policy, not a provider fact.

### E.3.6 Encoding rules are stated for query params but not for paths — OPEN

D.4 is right about `facets` / `ids` / `loaders`. Unstated: Modrinth **slugs** go in the path
(`/v2/project/{id|slug}`) and are `[a-zA-Z0-9!@$()`.+,_"-]{3,64}` per the spec — the
backtick, `"` and `+` in that class all need percent-encoding in a path segment, and
`Url::join` will not do it for you if you build the string yourself.
**Where to look:** https://docs.modrinth.com/openapi.yaml (`ProjectIdentifier` pattern).

## E.4 The five that matter most

1. **E.3.1 — the CurseForge update-check mechanism.** **RESOLVED, and it invalidates plan
   D7.** `exactMatches[].latestFiles` is unfiltered by MC version and loader (11 entries
   spanning 1.12.2–1.21.1 and three loaders for a 1.20.1 Forge fingerprint, with **no 1.20.1
   entry at all**), so there is no one-request CurseForge update check. It is N requests —
   one `GET /v1/mods/{id}/files?gameVersion=<mc>` per installed mod — exactly as Prism does.
2. **E.2.1 — `download-info`.** RESOLVED, and it improves R3: a 200 with sha1 for a *blocked*
   file, where `/download-url` gives a bare 403.
3. **E.1.5 + E.1.4 — mod vs modpack on Modrinth.** RESOLVED: the `project_type:` facet is
   plural-matched and will hand a mod browser the occasional modpack; filter the hit's own
   `project_type`. CurseForge has no equivalent problem (E.2.6).
4. **B.4 "Recovering the loader when `modLoader` is null"** (folded into §B this pass).
   RESOLVED: `sortableGameVersions[].gameVersionTypeId == 68441` is the loader row, `75208`
   is the side row — from `GET /v1/games/432/version-types`. Without it, every CurseForge
   file whose `modLoader` is null (common, B.2) is unclassifiable.
5. **E.3.2 — timeouts and retry semantics.** **OPEN** and entirely unevidenced, while the
   plan states specific numbers. Modrinth sends no `Retry-After` and CurseForge sends no
   rate-limit signal at all, so the retry policy is the one place where "we guessed" has to
   be written down as a guess.
