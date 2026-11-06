#ifndef GCAD_MESH_H
#define GCAD_MESH_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct GcadModel GcadModel;

GcadModel *gcad_build(const char *source, double detail, int y_up);
void gcad_free(GcadModel *model);
const char *gcad_error(const GcadModel *model);

size_t gcad_body_count(const GcadModel *model);
size_t gcad_vertex_count(const GcadModel *model, size_t body);
size_t gcad_index_count(const GcadModel *model, size_t body);
size_t gcad_joint_count(const GcadModel *model, size_t body);

const float *gcad_positions(const GcadModel *model, size_t body);
const float *gcad_normals(const GcadModel *model, size_t body);
const float *gcad_colours(const GcadModel *model, size_t body);
const uint32_t *gcad_indices(const GcadModel *model, size_t body);
const uint16_t *gcad_joint_indices(const GcadModel *model, size_t body);
const float *gcad_joint_weights(const GcadModel *model, size_t body);

size_t gcad_clip_count(const GcadModel *model, size_t body);
float gcad_clip_duration(const GcadModel *model, size_t body, size_t clip);

size_t gcad_bone_matrices(const GcadModel *model, size_t body, int clip, double t,
                          float *out, size_t capacity);

uint8_t *gcad_glb(const GcadModel *model, size_t *len);
void gcad_glb_free(uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif

#endif
