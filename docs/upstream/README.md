# Upstream issues

Tracked dependency bugs Metis cannot fix in-tree.

## wayland-rs — server/sys ObjectData UAF

- **Issue:** [Smithay/wayland-rs#949](https://github.com/Smithay/wayland-rs/issues/949)
- Local body copy: [`wayland-rs-server-objectdata-uaf.md`](wayland-rs-server-objectdata-uaf.md)
- Blocks default-on `wp_color_management_v1` (`METIS_COLOR_MGMT=1` remains opt-in)
