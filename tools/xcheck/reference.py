import sys
import gmsh
import numpy as np
from scipy.sparse.linalg import eigsh
from skfem import (Basis, ElementTetP2, ElementVector, FacetBasis, MeshTet,
                   BilinearForm, LinearForm, asm)
from skfem.helpers import dot, grad
from skfem.models.elasticity import linear_elasticity, lame_parameters
from fast import inverse, solve_free


def load(step, size):
    gmsh.initialize()
    gmsh.option.setNumber("General.Terminal", 0)
    gmsh.model.occ.importShapes(step)
    gmsh.model.occ.synchronize()
    gmsh.option.setNumber("Mesh.MeshSizeMax", size)
    gmsh.option.setNumber("Mesh.MeshSizeFromCurvature", 12)
    gmsh.model.mesh.generate(3)
    tags, coords, _ = gmsh.model.mesh.getNodes()
    index = {t: i for i, t in enumerate(tags)}
    points = coords.reshape(-1, 3)
    _, cells = gmsh.model.mesh.getElementsByType(4)
    tets = np.array([index[t] for t in cells]).reshape(-1, 4)
    used = np.unique(tets)
    remap = -np.ones(len(points), dtype=int)
    remap[used] = np.arange(len(used))
    mesh = MeshTet(points[used].T, remap[tets].T)
    surfaces = []
    for dim, tag in gmsh.model.getEntities(2):
        box = gmsh.model.getBoundingBox(dim, tag)
        surfaces.append((tag, np.array(box)))
    gmsh.finalize()
    return mesh, surfaces


def facets_where(mesh, test):
    b = mesh.boundary_facets()
    mid = mesh.p[:, mesh.facets[:, b]].mean(axis=1)
    return b[test(mid)]


def bracket_static(size):
    mesh, _ = load("bracket.step", size)
    e = ElementVector(ElementTetP2())
    basis = Basis(mesh, e)
    K = asm(linear_elasticity(*lame_parameters(69.0e3, 0.33)), basis)
    bottom = facets_where(mesh, lambda m: m[2] < 1e-6)
    top = facets_where(mesh, lambda m: (abs(m[2] - 16.0) < 1e-6) & (np.hypot(m[0] - 18, m[1] - 8) < 6.01))
    fb = FacetBasis(mesh, e, facets=top)
    area = asm(LinearForm(lambda v, w: v), FacetBasis(mesh, ElementTetP2(), facets=top)).sum()
    traction = np.array([0.0, -200.0 / area, 0.0])
    f = asm(LinearForm(lambda v, w: traction[0] * v[0] + traction[1] * v[1] + traction[2] * v[2]), fb)
    fixed = basis.get_dofs(bottom).all()
    u = solve_free(K, f, basis.complement_dofs(fixed))
    probe = np.array([[18.0], [12.0], [12.0]])
    d = basis.probes(probe) @ u
    lam, mu = lame_parameters(69.0e3, 0.33)
    sb = basis.with_element(ElementTetP2())

    def component(i, j):
        @BilinearForm
        def mass(a, v, w):
            return a * v

        @LinearForm
        def load(v, w):
            g = w["u"].grad
            eps = 0.5 * (g[i, j] + g[j, i])
            tr = g[0, 0] + g[1, 1] + g[2, 2]
            return (2 * mu * eps + (lam * tr if i == j else 0)) * v

        return solve_free(asm(mass, sb), asm(load, sb, u=basis.interpolate(u)), np.arange(sb.N))

    s = {(i, j): sb.probes(probe) @ component(i, j) for i in range(3) for j in range(i, 3)}
    sx, sy, sz = s[0, 0], s[1, 1], s[2, 2]
    vm = np.sqrt(0.5 * ((sx - sy) ** 2 + (sy - sz) ** 2 + (sz - sx) ** 2) + 3 * (s[0, 1] ** 2 + s[1, 2] ** 2 + s[0, 2] ** 2))
    nodal = u[basis.nodal_dofs].T
    peak = np.max(np.linalg.norm(nodal, axis=1))
    print(f"bracket static size {size}: {mesh.t.shape[1]} tets, {len(u)} dofs")
    print(f"  displacement max {peak:.6f} mm, top.displacement {np.linalg.norm(d):.6f} mm, top.stress {vm[0]:.4f} MPa")


def bracket_modal(size):
    mesh, _ = load("bracket.step", size)
    e = ElementVector(ElementTetP2())
    basis = Basis(mesh, e)
    K = asm(linear_elasticity(*lame_parameters(69.0e3, 0.33)), basis)

    @BilinearForm
    def mass(u, v, w):
        return 2.7e-9 * dot(u, v)

    M = asm(mass, basis)
    bottom = facets_where(mesh, lambda m: m[2] < 1e-6)
    fixed = basis.get_dofs(bottom).all()
    I = basis.complement_dofs(fixed)
    KI = K[I][:, I]
    values, _ = eigsh(KI, k=4, M=M[I][:, I], sigma=0.0, OPinv=inverse(KI))
    print(f"bracket modal size {size}: {len(I)} free dofs")
    print("  " + ", ".join(f"{np.sqrt(v) / (2 * np.pi):.1f} Hz" for v in sorted(values)))


def heatsink(size):
    mesh, _ = load("heatsink.step", size)
    e = ElementTetP2()
    basis = Basis(mesh, e)
    k, h, ambient = 0.167, 10.0e-6, 25.0

    @BilinearForm
    def conduct(u, v, w):
        return k * dot(grad(u), grad(v))

    bottom = facets_where(mesh, lambda m: m[2] < 1e-6)
    base_top = facets_where(mesh, lambda m: abs(m[2] - 4.0) < 1e-6)
    every = mesh.boundary_facets()
    air = np.setdiff1d(every, np.concatenate([bottom, base_top]))
    fa, fb = FacetBasis(mesh, e, facets=air), FacetBasis(mesh, e, facets=bottom)

    @BilinearForm
    def film(u, v, w):
        return h * u * v

    @LinearForm
    def one(v, w):
        return v

    area = asm(one, fb).sum()
    A = asm(conduct, basis) + asm(film, fa)
    b = ambient * h * asm(one, fa) + (5.0 / area) * asm(one, fb)
    T = solve_free(A, b, np.arange(A.shape[0]))
    print(f"heatsink size {size}: {len(T)} dofs, hottest {T.max():.3f} C, coolest {T.min():.3f} C")


which = sys.argv[1]
size = float(sys.argv[2])
{"static": bracket_static, "modal": bracket_modal, "heat": heatsink}[which](size)
