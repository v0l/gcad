# bvh-retarget

Retargets motion capture takes (BVH) onto a gcad rig and writes them as `clip` and `key` lines, so
a mesh file can `include` them and `gcad export` writes the clips into its GLB.

```sh
python3 tools/bvh-retarget/retarget.py config.json motion.gcad
```

Needs Python 3 and numpy. The rig is read from the `bone` and `mirror x of=` lines of a gcad file.
Each rig bone takes the rotation of one source joint, aimed from that joint to a second one, so a
rig with fewer bones than the take (15 against 60, say) still follows it.

## Config

Paths are relative to the config file.

| key | what it is |
|---|---|
| `rig` | the gcad file with the `bone` lines |
| `map` | rig bone to `[source joint, joint it points at]`. Every rig bone needs one |
| `root` | the rig's root bone; it gets the `move` key that lifts the body |
| `scale` | `{"bone", "source", "floor"}`: the rig bone whose head height matches the source joint's height above the `floor` joints. Sets how far the root moves |
| `facing` | two source joints across the body, left then right, that set which way the take faces |
| `aim` | optional, rig bone to `[from, to]` source joints it aims along instead (spine and head) |
| `upright` | optional, rig bone to `[keep, limit]`: scale its forward lean by `keep`, at most `limit` degrees |
| `arms` | optional, upper arm bone to side (`1` left, `-1` right): keep it at least `arm_clear` degrees off the chest |
| `contacts` | `[bone, "head"\|"tail"\|"mid", pad]` points that may touch the ground, `pad` in rig units |
| `loop_bones` | bones compared when finding a loop, all of them unless given |
| `fps` | key rate, 30 unless given |
| `clips` | clip name to its take, below |

A clip has `file` (the BVH), and optionally `trim` `[first, last)` frames, `loop` (find the closest
matching pair of frames inside `search` `[from, to]` fractions of the take, `span` `[min, max]`
frames apart, and close the loop there), `amp` (bone to a factor that scales its rotation), `lift`
(`ground` keeps the take's own height, `plant` puts the lowest contact on the floor every frame,
anything else leaves it), `upright: false` and `arm_clear`.

`hero.json` is the config of the kids-free-games runner hero (`art/3d/hero`): it reproduces that
repo's `motion.gcad` byte for byte. `python3 tools/bvh-retarget/test_retarget.py` runs the tests.
