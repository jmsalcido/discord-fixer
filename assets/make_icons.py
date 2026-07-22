#!/usr/bin/env python3
"""Render the app icon: a blurple rounded square with a white refresh arrow.

Pure stdlib. Signed-distance fields give clean antialiasing with one sample per
pixel, so this stays fast enough to run in CI if the icon ever needs rebuilding.
"""
import math, struct, zlib, sys, os

BLURPLE_TOP = (0x64, 0x6F, 0xF5)
BLURPLE_BOT = (0x40, 0x4E, 0xD1)

def smoothstep(edge, d, w):
    """Coverage in [0,1] for a signed distance d, edge at `edge`, width w."""
    t = (edge - d) / w + 0.5
    return 0.0 if t <= 0 else 1.0 if t >= 1 else t * t * (3 - 2 * t)

def sd_round_box(px, py, hx, hy, r):
    qx, qy = abs(px) - hx + r, abs(py) - hy + r
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - r

def sd_arc(px, py, radius, half_thick, a0, a1):
    """Annulus sector between angles a0..a1 (radians, CCW)."""
    ang = math.atan2(py, px) % (2 * math.pi)
    a0 %= 2 * math.pi
    a1 %= 2 * math.pi
    inside = a0 <= ang <= a1 if a0 <= a1 else (ang >= a0 or ang <= a1)
    ring = abs(math.hypot(px, py) - radius) - half_thick
    if inside:
        return ring
    # Outside the sweep: fall back to distance from the nearer cap.
    best = 1e9
    for a in (a0, a1):
        best = min(best, math.hypot(px - radius * math.cos(a), py - radius * math.sin(a)) - half_thick)
    return max(ring, best)

def sd_triangle(px, py, pts):
    """Signed distance to a convex triangle (max of the three half-planes)."""
    d = -1e9
    cx = sum(p[0] for p in pts) / 3
    cy = sum(p[1] for p in pts) / 3
    for i in range(3):
        ax, ay = pts[i]
        bx, by = pts[(i + 1) % 3]
        ex, ey = bx - ax, by - ay
        nx, ny = ey, -ex
        n = math.hypot(nx, ny) or 1.0
        nx, ny = nx / n, ny / n
        if (cx - ax) * nx + (cy - ay) * ny > 0:  # point normals outward
            nx, ny = -nx, -ny
        d = max(d, (px - ax) * nx + (py - ay) * ny)
    return d

def render(size):
    px_w = 2.0 / size            # one pixel in unit space, for AA width
    buf = bytearray(size * size * 4)

    R, THICK = 0.46, 0.088
    A0, A1 = math.radians(200), math.radians(110)   # sweep with a gap top-right
    tip_ang = math.radians(200)
    dirx, diry = math.sin(tip_ang), -math.cos(tip_ang)      # clockwise tangent
    perpx, perpy = -diry, dirx
    ex, ey = R * math.cos(tip_ang), R * math.sin(tip_ang)
    # The base sits back inside the arc so the two shapes fuse without a notch.
    tri = [
        (ex + dirx * 0.19, ey + diry * 0.19),
        (ex - dirx * 0.07 + perpx * 0.165, ey - diry * 0.07 + perpy * 0.165),
        (ex - dirx * 0.07 - perpx * 0.165, ey - diry * 0.07 - perpy * 0.165),
    ]

    for j in range(size):
        y = 1.0 - 2.0 * (j + 0.5) / size
        t = j / (size - 1)
        bg = tuple(int(BLURPLE_TOP[c] + (BLURPLE_BOT[c] - BLURPLE_TOP[c]) * t) for c in range(3))
        row = j * size * 4
        for i in range(size):
            x = 2.0 * (i + 0.5) / size - 1.0

            bg_cov = smoothstep(0.0, sd_round_box(x, y, 0.88, 0.88, 0.30), px_w * 1.5)
            if bg_cov <= 0.0:
                continue

            glyph = min(sd_arc(x, y, R, THICK, A0, A1), sd_triangle(x, y, tri))
            gl_cov = smoothstep(0.0, glyph, px_w * 1.5)

            r = int(bg[0] + (255 - bg[0]) * gl_cov)
            g = int(bg[1] + (255 - bg[1]) * gl_cov)
            b = int(bg[2] + (255 - bg[2]) * gl_cov)
            o = row + i * 4
            buf[o:o + 4] = bytes((r, g, b, int(255 * bg_cov)))
    return bytes(buf)

def write_png(path, size, rgba):
    raw = b"".join(b"\x00" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    open(path, "wb").write(png)

def write_ico(path, pngs):
    """ICO with embedded PNGs (supported since Vista)."""
    n = len(pngs)
    header = struct.pack("<HHH", 0, 1, n)
    offset = 6 + 16 * n
    entries, blobs = b"", b""
    for size, data in pngs:
        entries += struct.pack("<BBBBHHII", size & 0xFF, size & 0xFF, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    open(path, "wb").write(header + entries + blobs)

if __name__ == "__main__":
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)

    rgba1024 = render(1024)
    write_png(os.path.join(out, "icon.png"), 1024, rgba1024)

    # Raw RGBA for the runtime window icon — avoids pulling in a PNG decoder.
    open(os.path.join(out, "icon-256.rgba"), "wb").write(render(256))

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    pngs = []
    for s in ico_sizes:
        tmp = os.path.join(out, f".ico-{s}.png")
        write_png(tmp, s, render(s))
        pngs.append((s, open(tmp, "rb").read()))
        os.remove(tmp)
    write_ico(os.path.join(out, "icon.ico"), pngs)
    print("wrote icon.png (1024), icon-256.rgba, icon.ico")
