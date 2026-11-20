"""Writes p3-test.icc: a minimal matrix/TRC RGB profile with Display-P3 primaries and a 1024-entry sRGB-like curve.

The table curve keeps libjxl from turning it into a named colour space, so the .jxl really carries an ICC profile.
Run by make-jxl.sh. Standard library only."""
import struct


def s15(x):
    return struct.pack('>i', int(round(x * 65536)))


def xyz(x, y, z):
    return b'XYZ \0\0\0\0' + s15(x) + s15(y) + s15(z)


def text(t):
    return b'text\0\0\0\0' + t + b'\0'


def desc(t):
    a = t.encode() + b'\0'
    return b'desc\0\0\0\0' + struct.pack('>I', len(a)) + a + b'\0' * (4 + 4 + 2 + 1 + 67)


def curv():
    n = 1024
    v = []
    for i in range(n):
        x = i / (n - 1)
        y = x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4
        v.append(int(round(y * 65535)))
    return b'curv\0\0\0\0' + struct.pack('>I', n) + struct.pack('>%dH' % n, *v)


c = curv()
tags = [
    ('desc', desc('P3 test')), ('cprt', text(b'none')), ('wtpt', xyz(0.9642, 1.0, 0.8249)),
    ('rXYZ', xyz(0.5151, 0.2412, -0.0011)), ('gXYZ', xyz(0.2920, 0.6922, 0.0419)), ('bXYZ', xyz(0.1571, 0.0666, 0.7841)),
    ('rTRC', c), ('gTRC', c), ('bTRC', c),
]
n = len(tags)
off = 128 + 4 + 12 * n
table = b''
body = b''
cache = {}
for sig, d in tags:
    if d in cache:
        o, l = cache[d]
    else:
        body += b'\0' * ((-len(body)) % 4)
        o = off + len(body)
        l = len(d)
        body += d
        cache[d] = (o, l)
    table += sig.encode() + struct.pack('>II', o, l)
body += b'\0' * ((-len(body)) % 4)
size = 128 + 4 + 12 * n + len(body)
h = struct.pack('>I', size) + b'\0\0\0\0' + struct.pack('>I', 0x02100000) + b'mntrRGB XYZ ' + b'\0' * 12 + b'acsp'
h = h.ljust(68, b'\0') + s15(0.9642) + s15(1.0) + s15(0.8249)
h = h.ljust(128, b'\0')
with open('p3-test.icc', 'wb') as f:
    f.write(h + struct.pack('>I', n) + table + body)
