import sys
import gmsh
import numpy as np
from skfem import (Basis, ElementTetP2, ElementVector, MeshTet, BilinearForm,
                   LinearForm, asm)
from skfem.helpers import ddot, sym_grad, eye, trace
from skfem.models.elasticity import lame_parameters
from fast import solve_free

size = float(sys.argv[1])
gmsh.initialize()
gmsh.option.setNumber("General.Terminal", 0)
volumes = gmsh.model.occ.importShapes("joint.step")
gmsh.model.occ.fragment(volumes[:1], volumes[1:])
gmsh.model.occ.synchronize()
gmsh.option.setNumber("Mesh.MeshSizeMax", size)
gmsh.model.mesh.generate(3)
tags, coords, _ = gmsh.model.mesh.getNodes()
index = {t: i for i, t in enumerate(tags)}
points = coords.reshape(-1, 3)
tets, kinds = [], []
for dim, tag in gmsh.model.getEntities(3):
    lo = np.array(gmsh.model.getBoundingBox(dim, tag))
    width = lo[3] - lo[0]
    kind = "plate" if width > 39 else ("rod" if width < 9 else "tube")
    _, _, nodes = gmsh.model.mesh.getElements(dim, tag)
    block = np.array([index[t] for t in nodes[0]]).reshape(-1, 4)
    tets.append(block)
    kinds += [kind] * len(block)
gmsh.finalize()
tets = np.vstack(tets)
used = np.unique(tets)
remap = -np.ones(len(points), dtype=int)
remap[used] = np.arange(len(used))
mesh = MeshTet(points[used].T, remap[tets].T)
kinds = np.array(kinds)
e = ElementVector(ElementTetP2())
moduli = {"plate": 200.0e3, "rod": 200.0e3, "tube": 69.0e3}
ratios = {"plate": 0.29, "rod": 0.29, "tube": 0.33}
K = 0
f = 0
for kind in moduli:
    part = Basis(mesh, e, elements=np.nonzero(kinds == kind)[0])
    lam, mu = lame_parameters(moduli[kind], ratios[kind])

    @BilinearForm
    def stiff(u, v, w):
        eu = sym_grad(u)
        return ddot(2 * mu * eu + lam * eye(trace(eu), 3), sym_grad(v))

    K = K + asm(stiff, part)
    if kind == "rod":
        free = 12.0e-6 * -100.0

        @LinearForm
        def thermal(v, w):
            return (3 * lam + 2 * mu) * free * trace(sym_grad(v))

        f = f + asm(thermal, part)
basis = Basis(mesh, e)
b = mesh.boundary_facets()
mid = mesh.p[:, mesh.facets[:, b]].mean(axis=1)
bottom = b[mid[2] < -20 + 1e-6]
left = b[(mid[0] < -20 + 1e-6) & (mid[2] < 0)]
front = b[(mid[1] < -20 + 1e-6) & (mid[2] < 0)]
fixed = np.concatenate([
    basis.get_dofs(bottom).all("u^3"),
    basis.get_dofs(left).all("u^1"),
    basis.get_dofs(front).all("u^2"),
])
u = solve_free(K, f, basis.complement_dofs(fixed))


for name, point, kind in [("mid", (0.0, 0.0, 25.0), "rod"), ("wall", (13.5, 0.0, 25.0), "tube")]:
    lam, mu = lame_parameters(moduli[kind], ratios[kind])
    free = 12.0e-6 * -100.0 if kind == "rod" else 0.0
    eps = 1e-4
    p = np.array(point)
    probe = lambda q: basis.probes(q.reshape(3, 1)) @ u
    grad_u = np.zeros((3, 3))
    for j in range(3):
        d = np.zeros(3)
        d[j] = eps
        grad_u[:, j] = (probe(p + d) - probe(p - d)) / (2 * eps)
    strain = 0.5 * (grad_u + grad_u.T) - free * np.eye(3)
    sigma = 2 * mu * strain + lam * np.trace(strain) * np.eye(3)
    dev = sigma - np.trace(sigma) / 3 * np.eye(3)
    vm = np.sqrt(1.5 * np.sum(dev * dev))
    print(f"{name}.stress {vm:.3f} MPa, displacement {np.linalg.norm(probe(p)):.5f} mm")
print(f"{mesh.t.shape[1]} tets, {len(u)} dofs")
