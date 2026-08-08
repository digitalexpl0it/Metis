# Metis widget pack schema (API 1)

Author guide for declarative desktop widget extensions (Phase 14 §E).
Runtime validation is **fail-closed** at discovery (Phase 18 C): invalid packs are
skipped with a warning (and a toast at widgets-host startup). They never appear
in Settings → Desktop widgets → Add, and they must not crash the host.

See also: [User Guide — Desktop widgets](USER_GUIDE.md) (Extensions subsection).

## Install layout

```text
~/.local/share/metis/widgets/<id>/
  manifest.json
  widget.json
  helper                 # optional basename only
```

Also searched (lower priority than the user dir):

- `/usr/local/share/metis/widgets/<id>/`
- `/usr/share/metis/widgets/<id>/`

Rules:

- Folder name **must** equal `manifest.id` (reverse-DNS, lowercase, contains `.`).
- Both JSON files are required.
- No scripts, `.so`, or WASM — layout + optional helper binary only.
- Unknown JSON fields are **rejected** (`deny_unknown_fields`).

Example packs in-tree: `metis-os-workspace/assets/widgets/com.metis.example.quicklinks/`,
`…/com.metis.example.helperstatus/`.

## `manifest.json`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `id` | string | yes | Extension id; must match folder name |
| `name` | string | yes | Display name in Settings |
| `version` | string | no | Default `1.0.0` |
| `api` | number | no | Must be `1` (`WIDGET_EXT_API`) |
| `default_size` | `[w, h]` | no | Default `[320, 200]` |
| `min_size` | `[w, h]` | no | Optional minimum |
| `settings_schema` | array | no | See below |
| `helper` | object | no | See Helper |

### Settings schema entries

| Field | Type | Notes |
|-------|------|-------|
| `key` | string | Non-empty; no `/`, `\`, or `..` |
| `type` | `"string"` \| `"bool"` \| `"number"` | Default `string` |
| `label` | string | UI label |
| `default` | JSON value | Typed default |

## `widget.json`

Root is one layout **node**. Node types (`type` tag):

| `type` | Fields |
|--------|--------|
| `column` / `row` / `list` | `spacing` (int, default 0), `children` (array of nodes) |
| `label` | `text`, `style` (`body` \| `title` \| `muted`) |
| `icon` | `name` (theme icon, no paths), `pixel_size` (default 24) |
| `button` | `label`, `on_click` (action object) |
| `separator` | (no fields) |

### Caps

| Cap | Limit |
|-----|-------|
| File size | 256 KiB |
| Nesting depth | 12 |
| Total nodes | 256 |
| Label / URI strings | 2048 chars |
| `copy_text` payload | 4096 chars |

### Actions (`on_click`)

Tagged with `action`:

| `action` | Fields | Rules |
|----------|--------|-------|
| `open_uri` | `uri` | **http/https only**; no `file:`, userinfo, or custom schemes |
| `launch` | `id` and/or `exec` | Desktop id **or** single PATH basename; no argv/paths; interpreters denylisted |
| `copy_text` | `text` | Length-capped |

Placeholders:

- Labels / copy text: `{settings.<key>}`, `{host.time}`, `{host.date}`, `{host.weather.*}`, `{host.sys.*}`, `{helper.<key>}`
- **Not** allowed in `open_uri` / `launch` targets (actions are not settings-interpolated)

## Helper (optional)

In `manifest.json`:

```json
"helper": {
  "exec": "helper",
  "poll_seconds": 5
}
```

- `exec` is a **basename** under the pack root (no `/`, no `..`).
- Discovery requires the file to exist and stay under the pack after `canonicalize`.
- Host runs argv-only, cleared env (`PATH`/`HOME`/`LANG` only), timeout 3s, stdout ≤ 8 KiB.
- Stdout must be a JSON object; string/number/bool values become `{helper.<key>}` in labels.

## Discovery behavior

1. Parse `manifest.json` (strict).
2. Validate id / folder / `api` / settings keys.
3. Parse + validate `widget.json` (strict + action hardening).
4. If `helper` is set, resolve the helper binary under the pack root.
5. On any failure: **skip** the pack (`tracing::warn`), continue scanning.
6. Widgets host startup: one toast if any packs were skipped.

Configured instances that reference a missing/skipped pack show an in-card error UI; the host keeps running.

## Versioning

Bump `api` only when Metis ships a new host contract. Packs with a mismatched
`api` are rejected at discovery.
