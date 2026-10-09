import numpy as np
import pypardiso
from scipy.sparse.linalg import LinearOperator


def solve_free(A, b, free):
    out = np.zeros(A.shape[0])
    out[free] = pypardiso.spsolve(A.tocsr()[free][:, free].tocsr(), b[free])
    return out


def inverse(A):
    solver = pypardiso.PyPardisoSolver()
    A = A.tocsr()
    solver.factorize(A)
    return LinearOperator(A.shape, matvec=lambda x: solver.solve(A, np.asarray(x).ravel()))
