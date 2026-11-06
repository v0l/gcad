#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "gcad_mesh.h"

static int fail(const char *what) {
    fprintf(stderr, "FAIL %s\n", what);
    return 1;
}

int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "rb");
    if (!f) return fail("open");
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *src = malloc(n + 1);
    if (fread(src, 1, n, f) != (size_t)n) return fail("read");
    src[n] = 0;
    fclose(f);

    GcadModel *m = gcad_build(src, 1.0, 1);
    if (gcad_error(m)) { fprintf(stderr, "%s\n", gcad_error(m)); return fail("build"); }
    if (gcad_body_count(m) != 1) return fail("bodies");
    size_t verts = gcad_vertex_count(m, 0), idx = gcad_index_count(m, 0), joints = gcad_joint_count(m, 0);
    if (verts < 100 || idx < 300 || idx % 3 || joints != 14) return fail("counts");
    if (!gcad_positions(m, 0) || !gcad_normals(m, 0) || !gcad_colours(m, 0) || !gcad_indices(m, 0)
        || !gcad_joint_indices(m, 0) || !gcad_joint_weights(m, 0)) return fail("buffers");
    for (size_t i = 0; i < idx; i++) if (gcad_indices(m, 0)[i] >= verts) return fail("index range");

    size_t needed = gcad_bone_matrices(m, 0, 0, 0.0, NULL, 0);
    if (needed != joints) return fail("matrix count");
    float *mats = malloc(needed * 16 * sizeof(float));
    gcad_bone_matrices(m, 0, 1, 0.25, mats, needed * 16);
    for (size_t i = 0; i < needed * 16; i++) if (!isfinite(mats[i])) return fail("matrix finite");
    if (gcad_clip_count(m, 0) != 2 || gcad_clip_duration(m, 0, 0) <= 0.0f) return fail("clips");

    size_t len = 0;
    uint8_t *glb = gcad_glb(m, &len);
    if (!glb || len < 1000 || memcmp(glb, "glTF", 4)) return fail("glb");
    gcad_glb_free(glb, len);
    gcad_free(m);

    GcadModel *bad = gcad_build("a: blob 0,0,0 r=2\nrect 1 1\n", 1.0, 1);
    const char *e = gcad_error(bad);
    if (!e || !strstr(e, "line 2")) return fail("error text");
    if (gcad_body_count(bad) != 0) return fail("bad bodies");
    gcad_free(bad);
    free(mats);
    free(src);
    printf("c abi ok: %zu vertices, %zu indices, %zu joints\n", verts, idx, joints);
    return 0;
}
