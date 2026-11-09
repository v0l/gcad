import os
import re
import subprocess

import numpy as np
from PIL import Image, ImageDraw

VIEWS = {
    "front": (0, 2, 1),
    "back": (0, 2, -1),
    "right": (1, 2, 1),
    "left": (1, 2, -1),
    "top": (0, 1, 1),
    "bottom": (0, 1, -1),
}
SIL = 256
PRIMITIVES = {"blob", "limb", "block"}
MESH_OPS = {
    "remesh", "smooth", "inflate", "refine", "crease", "pinch", "flatten", "noise", "relax", "decimate",
    "bone", "skin", "clip", "key", "brush", "stroke", "dab", "mask", "dyntopo", "subdivide", "level",
    "project", "layer", "finish", "bake", "lod", "select", "group", "join",
}
SYMMETRY_MIN = 0.85
OP_LINE = re.compile(r"^\s*(?:[A-Za-z_][\w]*\s*:\s*)?([a-z_]+)\b")


def gcad(*args, timeout=300):
    return subprocess.run(["gcad", *args], capture_output=True, text=True, timeout=timeout)


def check(path):
    try:
        p = gcad("check", path)
    except subprocess.TimeoutExpired:
        return {"ok": False, "error": "check timed out"}
    out = p.stdout + p.stderr
    errors = [ln.strip() for ln in out.splitlines() if "ERROR" in ln or ln.startswith("Error")]
    return {"ok": p.returncode == 0 and not errors, "error": errors[0][:300] if errors else ""}


def load_obj(path):
    verts, faces, groups = [], [], 0
    with open(path) as f:
        for ln in f:
            if ln.startswith("v "):
                p = ln.split()
                verts.append((float(p[1]), float(p[2]), float(p[3])))
            elif ln.startswith("f "):
                faces.append([int(t.split("/")[0]) - 1 for t in ln.split()[1:]])
            elif ln.startswith("g "):
                groups += 1
    return np.array(verts, dtype=float).reshape(-1, 3), faces, groups


def triangle_count(faces):
    return sum(max(len(f) - 2, 0) for f in faces)


def export_obj(gcad_path, out, max_tris=None):
    args = ["export", gcad_path, out]
    if max_tris:
        args += ["--tris", str(max_tris)]
    p = gcad(*args, timeout=600)
    return p.returncode == 0 and os.path.exists(out)


def silhouettes(verts, faces):
    lo, hi = verts.min(axis=0), verts.max(axis=0)
    centre = (lo + hi) / 2
    span = float((hi - lo).max()) or 1.0
    scale = (SIL - 8) / span
    masks = {}
    for name, (a, b, sign) in VIEWS.items():
        im = Image.new("1", (SIL, SIL), 0)
        d = ImageDraw.Draw(im)
        u = (verts[:, a] - centre[a]) * sign * scale + SIL / 2
        v = SIL / 2 - (verts[:, b] - centre[b]) * scale
        pts = np.stack([u, v], axis=1)
        for f in faces:
            if len(f) >= 3:
                d.polygon([tuple(pts[i]) for i in f], fill=1)
        masks[name] = np.array(im, dtype=bool)
    return masks


def perimeter(mask):
    p = np.pad(mask, 1)
    inner = p[1:-1, 1:-1] & p[:-2, 1:-1] & p[2:, 1:-1] & p[1:-1, :-2] & p[1:-1, 2:]
    return int((mask & ~inner).sum())


def complexity(mask):
    area = int(mask.sum())
    return perimeter(mask) ** 2 / area if area else 0.0


def iou(a, b):
    u = (a | b).sum()
    return float((a & b).sum() / u) if u else 0.0


def symmetry(masks):
    scores = [iou(masks[v], masks[v][:, ::-1]) for v in ("front", "top", "back", "bottom")]
    return float(np.mean(scores))


def extents(verts):
    d = verts.max(axis=0) - verts.min(axis=0)
    return {"x": float(d[0]), "y": float(d[1]), "z": float(d[2])}


def bbox_checks(dims, rules):
    out = []
    for r in rules:
        den = dims[r["den"]]
        ratio = dims[r["num"]] / den if den else 0.0
        out.append({"rule": f"{r['num']}/{r['den']}", "ratio": round(ratio, 3), "min": r["min"], "max": r["max"],
                    "pass": r["min"] <= ratio <= r["max"]})
    return out


def op_usage(text):
    ops = {}
    for ln in text.splitlines():
        m = OP_LINE.match(ln)
        if m:
            ops[m.group(1)] = ops.get(m.group(1), 0) + 1
    ops.pop("let", None)
    return ops


def mesh_op_report(ops):
    prim = sum(n for k, n in ops.items() if k in PRIMITIVES)
    adv = {k: n for k, n in ops.items() if k in MESH_OPS}
    return {"primitive_lines": prim, "mesh_ops": adv, "beyond_primitives": bool(adv)}


def measure(gcad_path, task, workdir):
    text = open(gcad_path).read() if os.path.exists(gcad_path) else ""
    ops = op_usage(text)
    res = {"lines": len([x for x in text.splitlines() if x.strip()]), "ops": ops, **mesh_op_report(ops)}
    c = check(gcad_path) if text else {"ok": False, "error": "no model.gcad written"}
    res["builds"] = c["ok"]
    res["build_error"] = c["error"]
    if not c["ok"]:
        res["hard_score"] = 0.0
        return res
    full = os.path.join(workdir, "full.obj")
    if not export_obj(gcad_path, full):
        res["builds"] = False
        res["build_error"] = "export failed"
        res["hard_score"] = 0.0
        return res
    verts, faces, groups = load_obj(full)
    res["triangles"] = triangle_count(faces)
    res["named_parts"] = max(groups, 1)
    if res["triangles"] > 150000:
        light = os.path.join(workdir, "light.obj")
        if export_obj(gcad_path, light, max_tris=100000):
            verts, faces, _ = load_obj(light)
    os.remove(full)
    if len(verts) == 0 or not faces:
        res["builds"] = False
        res["build_error"] = "empty model"
        res["hard_score"] = 0.0
        return res
    masks = silhouettes(verts, faces)
    comp = {v: round(complexity(m), 2) for v, m in masks.items()}
    res["silhouette_complexity"] = comp
    res["silhouette_complexity_mean"] = round(float(np.mean(list(comp.values()))), 2)
    dims = extents(verts)
    res["dims"] = {k: round(v, 3) for k, v in dims.items()}
    res["bbox"] = bbox_checks(dims, task.get("bbox", []))
    res["symmetry"] = round(symmetry(masks), 3)
    res["hard_score"] = hard_score(res, task)
    return res


def hard_score(res, task):
    if not res.get("builds"):
        return 0.0
    parts = [min(res["named_parts"] / task["min_parts"], 1.0)] if task.get("min_parts") else []
    if res["bbox"]:
        parts.append(sum(b["pass"] for b in res["bbox"]) / len(res["bbox"]))
    if task.get("symmetric"):
        parts.append(1.0 if res["symmetry"] >= SYMMETRY_MIN else res["symmetry"] / SYMMETRY_MIN)
    if task.get("kind") == "mesh":
        parts.append(1.0 if res["beyond_primitives"] else 0.0)
    return round(10 * float(np.mean(parts)), 2) if parts else 10.0
