import argparse
import json
import sys

import numpy as np
import trimesh

VOLUME_TOL = 0.01
CHAMFER_TOL = 0.005
SAMPLES = 20000


def load_mesh(path):
    loaded = trimesh.load(str(path), force="mesh", process=False)
    if isinstance(loaded, trimesh.Scene):
        loaded = trimesh.util.concatenate(list(loaded.geometry.values()))
    return loaded


def signed_volume(mesh):
    tri = mesh.triangles
    return float(np.einsum("ij,ij->", tri[:, 0], np.cross(tri[:, 1], tri[:, 2])) / 6.0)


def sample_surface(mesh, count, seed=0):
    points, _ = trimesh.sample.sample_surface(mesh, count, seed=seed)
    return np.asarray(points)


def one_sided(from_points, to_points):
    from scipy.spatial import cKDTree

    return cKDTree(to_points).query(from_points)[0]


def compare_meshes(a, b, volume_a=None, volume_b=None, samples=SAMPLES):
    va = abs(volume_a if volume_a is not None else signed_volume(a))
    vb = abs(volume_b if volume_b is not None else signed_volume(b))
    box_a, box_b = a.bounds, b.bounds
    diagonal = float(np.linalg.norm(box_a[1] - box_a[0]))
    pa = sample_surface(a, samples, 0)
    pb = sample_surface(b, samples, 1)
    d_ab = one_sided(pa, pb)
    d_ba = one_sided(pb, pa)
    chamfer = float((d_ab.mean() + d_ba.mean()) / 2.0) / diagonal
    hausdorff = float(max(d_ab.max(), d_ba.max())) / diagonal
    return {
        "volume_a": va,
        "volume_b": vb,
        "volume_error": abs(va - vb) / max(va, 1e-12),
        "bbox_error": float(np.abs(box_a - box_b).max()) / diagonal,
        "diagonal": diagonal,
        "chamfer": chamfer,
        "hausdorff": hausdorff,
    }


def verdict(metrics, volume_tol=VOLUME_TOL, chamfer_tol=CHAMFER_TOL):
    return metrics["volume_error"] <= volume_tol and metrics["chamfer"] <= chamfer_tol


def compare(path_a, path_b, **kwargs):
    return compare_meshes(load_mesh(path_a), load_mesh(path_b), **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("a")
    parser.add_argument("b")
    parser.add_argument("--volume-tol", type=float, default=VOLUME_TOL)
    parser.add_argument("--chamfer-tol", type=float, default=CHAMFER_TOL)
    args = parser.parse_args()
    metrics = compare(args.a, args.b)
    metrics["keep"] = verdict(metrics, args.volume_tol, args.chamfer_tol)
    print(json.dumps(metrics))
    sys.exit(0 if metrics["keep"] else 1)


if __name__ == "__main__":
    main()
