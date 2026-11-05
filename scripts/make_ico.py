#!/usr/bin/env python3
"""Packs PNG files into one Windows .ico (PNG-compressed entries).

    make_ico.py out.ico 16.png 24.png 32.png ...

Each PNG must be square and at most 256 px; the size is read from its header.
Used by scripts/icon.sh.
"""
import struct
import sys


def png_size(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("not a PNG file")
    width, height = struct.unpack(">II", data[16:24])
    if width != height or width > 256:
        raise SystemExit(f"icon sizes must be square and at most 256 px, got {width}x{height}")
    return width


def main(out_path, png_paths):
    images = []
    for path in png_paths:
        with open(path, "rb") as file:
            data = file.read()
        images.append((png_size(data), data))
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries = b""
    body = b""
    for size, data in images:
        # 0 in the width and height field means 256.
        dimension = 0 if size == 256 else size
        entries += struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(data), offset)
        body += data
        offset += len(data)
    with open(out_path, "wb") as file:
        file.write(header + entries + body)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2:])
