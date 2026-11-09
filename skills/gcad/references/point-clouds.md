# Point clouds

Status: nothing on master yet. Point cloud work is the chain #444 to #449: import (PLY, E57,
LAS), clean, register, segment, then reconstruct meshes or fit solids.

Until import lands, a scan cannot be read by gcad. Do not invent ops for it; check `gcad docs`
for a point cloud section first. Fill this file in as each issue lands, one section per stage,
with a `gcad` block that `gcad check` runs.
