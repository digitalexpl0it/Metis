# Decision: Metis-native remote host vs GRD (+ viewer)

**Status:** research / spike only (Optional stretch Wave 4b)  
**Date:** 2026-07-27  
**Default product host:** `gnome-remote-desktop` (GRD) via `metis-remote`  
**Related:** [UBUNTU_DEV.md](../UBUNTU_DEV.md), Phase 7 in `metis-os-workspace/TODO.md`

## Question

Should Metis replace GRD with a first-party remote **host** protocol (capture + input + transport), or keep investing in GRD + the first-party **viewer** (`metis-viewer`)?

## Building blocks already in-tree

| Concern | Existing path |
|--------|----------------|
| Capture | `metis-portal` ScreenCast + PipeWire dmabuf; compositor image-capture |
| Input inject | Compositor EIS / `remote_input.rs`; Mutter RemoteDesktop D-Bus shim in portal |
| Session share UX | Settings → Remote access; pause-on-lock; LAN firewall; credentials via `grdctl` |
| Client | `metis-viewer` (FreeRDP → GRD) |
| Stretch peer | Optional RustDesk backend (`metis-remote rustdesk …`) — not a Metis protocol |

## Spike options (not production)

1. **Portal pump latency** — ScreenCast dmabuf → encoder stub → discard; measure CPU/ms vs GRD.
2. **Compositor-direct** — DRM primary plane copy into a PipeWire stream (bypass portal); compare.
3. **Transport** — reuse RDP (FreeRDP server lib / NeutrinoRDP) vs WebRTC vs custom TLS framing.
4. **EIS-only inject** — already used; confirm lock-screen and multi-seat gaps vs Mutter RD.

No Wave 4b deliverable ships a wire protocol.

## Decision criteria (when to reopen)

Build a native host **only if** several of these remain true after GRD investment:

- Multi-monitor / HDR / VRR edge cases that GRD cannot express on Smithay.
- Clipboard or file-transfer formats users need that GRD will not grow.
- Licensing / packaging blocks GRD on a Metis ISO target.
- Portal + EIS path proves ≥2× better latency/CPU than GRD on the same hardware for the viewer we control.

Until then: **keep GRD as the default host**, polish viewer + portal, treat RustDesk as optional third-party.

## Recommendation (2026-07)

**Invest in GRD + viewer; defer native host.** Portal/EIS/ScreenCast already close the largest Smithay gaps that once motivated a native stack. A native host is multi-quarter and would duplicate Mutter RemoteDesktop surface area Metis already shims.

Revisit after a concrete GRD gap is documented with a failing QA case that portal/viewer work cannot fix.
