import sys
import time
import gmsh
import numpy as np
from scipy.sparse.linalg import eigsh
from skfem import (Basis, ElementTetP2, ElementVector, FacetBasis, MeshTet,
                   BilinearForm, LinearForm, asm)
from skfem.helpers import ddot, sym_grad, eye, trace
from skfem.models.elasticity import lame_parameters
from fast import inverse, solve_free

step, size, height = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
dry = len(sys.argv) > 4
start = time.time()
gmsh.initialize()
gmsh.option.setNumber("General.Terminal", 0)
gmsh.model.occ.importShapes(step)
gmsh.model.occ.synchronize()
gmsh.option.setNumber("Mesh.MeshSizeMax", size)
gmsh.model.mesh.generate(3)
tags, coords, _ = gmsh.model.mesh.getNodes()
index = {t: i for i, t in enumerate(tags)}
points = coords.reshape(-1, 3)
_, cells = gmsh.model.mesh.getElementsByType(4)
gmsh.finalize()
tets = np.array([index[t] for t in cells]).reshape(-1, 4)
used = np.unique(tets)
remap = -np.ones(len(points), dtype=int)
remap[used] = np.arange(len(used))
mesh = MeshTet(points[used].T, remap[tets].T)
e = ElementVector(ElementTetP2())
basis = Basis(mesh, e)
print(f"{mesh.t.shape[1]} tets, {basis.N} dofs")
if dry:
    sys.exit()
lam, mu = lame_parameters(200.0e3, 0.29)


@BilinearForm
def stiff(u, v, w):
    eu = sym_grad(u)
    return ddot(2 * mu * eu + lam * eye(trace(eu), 3), sym_grad(v))


K = asm(stiff, basis)
b = mesh.boundary_facets()
mid = mesh.p[:, mesh.facets[:, b]].mean(axis=1)
bottom, top = b[mid[2] < 1e-6], b[mid[2] > height - 1e-6]


@LinearForm
def one(v, w):
    return v


area = asm(one, FacetBasis(mesh, ElementTetP2(), facets=top)).sum()


@LinearForm
def push(v, w):
    return -1000.0 / area * v[2]


f = asm(push, FacetBasis(mesh, e, facets=top))
fixed = basis.get_dofs(bottom).all()
u = solve_free(K, f, basis.complement_dofs(fixed))


@BilinearForm
def geometric(du, dv, w):
    g = w["u"].grad
    eps = 0.5 * (g + np.swapaxes(g, 0, 1))
    tr = eps[0, 0] + eps[1, 1] + eps[2, 2]
    sigma = 2 * mu * eps + lam * eye(tr, 3)
    return np.einsum("ij...,ki...,kj...->...", sigma, du.grad, dv.grad)


G = asm(geometric, basis, u=basis.interpolate(u))
free = basis.complement_dofs(fixed)
Kf = K[free][:, free]
values = eigsh(Kf, k=3, M=-G[free][:, free], sigma=1.0, mode="buckling",
               OPinv=inverse(Kf - (-G[free][:, free])), return_eigenvectors=False)
print("buckling factors " + ", ".join(f"{v:.4f}" for v in sorted(values)))
print(f"{time.time() - start:.1f} s")
