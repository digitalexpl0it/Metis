# Upstream issue drafts

Paste-ready GitHub issue bodies for dependencies Metis cannot fix in-tree.

## wayland-rs — server/sys ObjectData UAF

- Body: [`wayland-rs-server-objectdata-uaf.md`](wayland-rs-server-objectdata-uaf.md)
- Title: `server/sys: ObjectData UAF on destroy + same-id reuse (wp_image_description_* / Chromium)`
- Repo: [Smithay/wayland-rs](https://github.com/Smithay/wayland-rs)

```bash
gh auth login   # once, if needed
gh issue create --repo Smithay/wayland-rs \
  --title "server/sys: ObjectData UAF on destroy + same-id reuse (wp_image_description_* / Chromium)" \
  --body-file docs/upstream/wayland-rs-server-objectdata-uaf.md
```

After filing, replace the draft link in `metis-os-workspace/TODO.md` Phase 5 §B
with the issue URL.
