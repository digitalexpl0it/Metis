#!/usr/bin/env python3
"""Generate a minimal but valid display ICC profile carrying a `vcgt` tag.

This exists purely to test Metis's per-output hardware-gamma calibration
(`metis-compositor/src/output_gamma.rs`) without a colorimeter. Real display
profiles get their `vcgt` from calibration hardware; this fakes one with an
obvious, reversible curve so you can *see* the compositor apply it.

Usage:
    python3 make-test-icc.py [OUTPUT.icc] [--profile warm|cool|dark|identity]

Default output: assets/test-<profile>-vcgt.icc next to the workspace.
Assign it in Settings -> Display -> Colour profile -> "Choose ICC profile…",
then press "Clear" to restore neutral. All ICC integers are big-endian.
"""

import argparse
import struct
import sys
from pathlib import Path


def s15f16(v: float) -> bytes:
    """Encode a float as an ICC s15Fixed16 (signed 16.16)."""
    return struct.pack(">i", round(v * 65536.0))


def xyz_type(x: float, y: float, z: float) -> bytes:
    return b"XYZ \x00\x00\x00\x00" + s15f16(x) + s15f16(y) + s15f16(z)


def curve_identity() -> bytes:
    # curveType with 0 entries == identity transfer function.
    return b"curv\x00\x00\x00\x00\x00\x00\x00\x00"


def text_desc(text: str) -> bytes:
    # ICC v2 textDescriptionType: ASCII + (empty) Unicode + (empty) ScriptCode.
    ascii_bytes = text.encode("ascii", "replace") + b"\x00"
    out = b"desc\x00\x00\x00\x00"
    out += struct.pack(">I", len(ascii_bytes))
    out += ascii_bytes
    out += struct.pack(">I", 0)      # Unicode language code
    out += struct.pack(">I", 0)      # Unicode count
    out += struct.pack(">H", 0)      # ScriptCode code
    out += struct.pack(">B", 0)      # ScriptCode count
    out += b"\x00" * 67              # ScriptCode description (fixed 67 bytes)
    return out


def vcgt_table(curves) -> bytes:
    """Build a `vcgt` tag (table encoding, 3 channels, 256 x 16-bit).

    `curves` is (r_fn, g_fn, b_fn); each maps input x in [0,1] to output [0,1].
    """
    n = 256
    body = b"vcgt"                    # tag type signature
    body += b"\x00\x00\x00\x00"       # reserved
    body += struct.pack(">I", 0)      # gamma type 0 == table
    body += struct.pack(">H", 3)      # channels
    body += struct.pack(">H", n)      # entries per channel
    body += struct.pack(">H", 2)      # bytes per entry (u16)
    for fn in curves:
        for i in range(n):
            x = i / (n - 1)
            y = min(1.0, max(0.0, fn(x)))
            body += struct.pack(">H", round(y * 65535))
    return body


PROFILES = {
    # Strong warm cast: leave red, pull green and blue down hard.
    "warm": (lambda x: x, lambda x: x * 0.75, lambda x: x * 0.40),
    # Strong cool cast: pull red down, leave blue.
    "cool": (lambda x: x * 0.45, lambda x: x * 0.80, lambda x: x),
    # Uniform darken via gamma — proves a non-linear curve, subtler.
    "dark": (lambda x: x ** 1.8, lambda x: x ** 1.8, lambda x: x ** 1.8),
    # Exact identity — should look like no profile at all (sanity check).
    "identity": (lambda x: x, lambda x: x, lambda x: x),
}


def build_icc(profile: str) -> bytes:
    desc = text_desc(f"Metis test vcgt ({profile})")
    tags = [
        (b"desc", desc),
        (b"cprt", text_desc("Metis test profile - not for real use")),
        (b"wtpt", xyz_type(0.9642, 1.0, 0.8249)),   # D50
        (b"rXYZ", xyz_type(0.436, 0.222, 0.014)),
        (b"gXYZ", xyz_type(0.385, 0.717, 0.097)),
        (b"bXYZ", xyz_type(0.143, 0.061, 0.714)),
        (b"rTRC", curve_identity()),
        (b"gTRC", curve_identity()),
        (b"bTRC", curve_identity()),
        (b"vcgt", vcgt_table(PROFILES[profile])),
    ]

    tag_count = len(tags)
    header_len = 128
    table_len = 4 + tag_count * 12
    offset = header_len + table_len
    # 4-byte align each tag's data start.
    entries = []
    data = b""
    for sig, body in tags:
        pad = (-offset) % 4
        data += b"\x00" * pad
        offset += pad
        entries.append((sig, offset, len(body)))
        data += body
        offset += len(body)

    total = offset
    header = bytearray(128)
    struct.pack_into(">I", header, 0, total)          # profile size
    struct.pack_into(">I", header, 8, 0x02400000)     # version 2.4
    header[12:16] = b"mntr"                            # device class: display
    header[16:20] = b"RGB "                            # data colour space
    header[20:24] = b"XYZ "                            # PCS
    header[36:40] = b"acsp"                            # required signature
    struct.pack_into(">i", header, 68, round(0.9642 * 65536))   # PCS illum X
    struct.pack_into(">i", header, 72, round(1.0 * 65536))      # PCS illum Y
    struct.pack_into(">i", header, 76, round(0.8249 * 65536))   # PCS illum Z

    table = struct.pack(">I", tag_count)
    for sig, off, ln in entries:
        table += sig + struct.pack(">II", off, ln)

    return bytes(header) + table + data


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("output", nargs="?", help="output .icc path")
    ap.add_argument("--profile", choices=sorted(PROFILES), default="warm")
    args = ap.parse_args()

    if args.output:
        out = Path(args.output)
    else:
        workspace = Path(__file__).resolve().parents[2]
        out = workspace / "assets" / f"test-{args.profile}-vcgt.icc"

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(build_icc(args.profile))
    print(f"wrote {out} ({out.stat().st_size} bytes, profile={args.profile})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
