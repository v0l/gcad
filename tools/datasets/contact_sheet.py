import random
import sys
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import trimesh
from common import *

SHEETS = DATA / "sheets"


ZUP = {"zero-to-cad", "cad-recode", "thingi10k"}


def render_ax(ax, glb, zup=False):
    scene = load_scene(DATA / glb)
    geoms = scene.dump(concatenate=False) if len(scene.graph.nodes_geometry) > 1 else list(scene.geometry.values())
    geoms = [g for g in geoms if isinstance(g, trimesh.Trimesh) and len(g.faces)]
    m = trimesh.util.concatenate(geoms)
    if len(m.faces) > 40000:
        idx = np.random.default_rng(0).choice(len(m.faces), 40000, replace=False)
        faces = m.faces[idx]
    else:
        faces = m.faces
    v = m.vertices - m.bounds.mean(0)
    v = v / max(float(np.abs(v).max()), 1e-9)
    ang = np.radians(35)
    rot = np.diag([1.0, -1.0, 1.0]) if zup else np.array([[1, 0, 0], [0, 0, -1], [0, 1, 0]], dtype=float)
    c, s = np.cos(ang), np.sin(ang)
    spin = np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])
    p = v @ rot.T @ spin.T
    tri = p[faces]
    n = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    nl = np.linalg.norm(n, axis=1, keepdims=True)
    n = n / np.where(nl == 0, 1, nl)
    light = np.array([0.4, -0.8, 0.5])
    light /= np.linalg.norm(light)
    shade = np.clip(np.abs(n @ light), 0, 1) * 0.75 + 0.2
    order = np.argsort(tri[:, :, 1].mean(1))[::-1]
    xy = tri[order][:, :, [0, 2]]
    from matplotlib.collections import PolyCollection
    ax.add_collection(PolyCollection(xy, facecolors=[(x, x, x) for x in shade[order]], edgecolors="none"))
    ax.set_xlim(-1.05, 1.05)
    ax.set_ylim(-1.05, 1.05)
    ax.set_aspect("equal")
    ax.axis("off")


def sheet(source, n=20):
    manifest = DATA / "out" / source / "manifest.jsonl"
    rows = [r for r in read_jsonl(manifest) if r["paths"].get("glb")]
    rng = random.Random(548)
    pick = rng.sample(rows, min(n, len(rows)))
    fig, axes = plt.subplots(4, 5, figsize=(15, 12.6))
    for ax in axes.flat:
        ax.axis("off")
    for ax, r in zip(axes.flat, pick):
        try:
            render_ax(ax, r["paths"]["glb"], source in ZUP)
            ax.set_title(r["id"].split(":", 1)[1][:22], fontsize=7)
        except Exception as e:
            ax.text(0.5, 0.5, "render failed", ha="center")
    fig.suptitle(f"{source}: 20 random objects", fontsize=12)
    SHEETS.mkdir(exist_ok=True)
    out = SHEETS / f"{source}.png"
    fig.savefig(out, dpi=70, bbox_inches="tight")
    plt.close(fig)
    print(out)


if __name__ == "__main__":
    for s in sys.argv[1:]:
        sheet(s)
