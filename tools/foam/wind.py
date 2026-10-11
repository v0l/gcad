import math, struct, sys

source, target, toward = sys.argv[1], sys.argv[2], [float(v) for v in sys.argv[3].split(",")]
n = math.sqrt(sum(v * v for v in toward))
x = [v / n for v in toward]
up = [0.0, 0.0, 1.0] if abs(x[2]) < 0.9 else [0.0, 1.0, 0.0]
d = sum(a * b for a, b in zip(up, x))
z = [u - d * a for u, a in zip(up, x)]
m = math.sqrt(sum(v * v for v in z))
z = [v / m for v in z]
y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]]
data = open(source, "rb").read()
count = struct.unpack_from("<I", data, 80)[0]
low, high = [math.inf] * 3, [-math.inf] * 3
out = bytearray(data[:84])
for t in range(count):
    values = struct.unpack_from("<12f", data, 84 + 50 * t)
    row = []
    for k in range(4):
        p = values[3 * k : 3 * k + 3]
        q = [sum(a * b for a, b in zip(axis, p)) for axis in (x, y, z)]
        if k > 0:
            q = [c * 0.001 for c in q]
            for i in range(3):
                low[i] = min(low[i], q[i])
                high[i] = max(high[i], q[i])
        row.extend(q)
    out += struct.pack("<12fH", *row, 0)
open(target, "wb").write(out)
print(" ".join(f"{v:.6f}" for v in low + high))
