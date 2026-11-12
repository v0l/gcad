# Synthetic concept images (#553)

Gives meshes with no concept art a FLUX concept image, filters the ones that drifted from the shape, and writes them into the manifest.

```
python3 pick.py --per-source 40 --out selection.jsonl
python3 render_all.py selection.jsonl
python3 flux_img2img.py selection.jsonl
python3 filter_iou.py --min-iou 0.75
python3 merge_manifest.py
python3 ablation.py
python3 -m unittest
```

Needs Blender (`~/gcad-datasets/tools`), numpy, scipy, Pillow, and ssh access to `spark`, where ComfyUI serves flux1-dev on port 8188. Renders run under `systemd-run -p MemoryMax=6G`: a few GLBs make Blender allocate tens of GB.

## Steps

1. `pick.py`: commercial_ok, gcad_fit high or medium, no concept yet, 50 to 400000 triangles, a fixed number per source, shuffled with a fixed seed.
2. `render_clay.py` (run by `render_all.py`): Blender Workbench, orthographic, azimuth 35 and elevation 25 degrees, transparent 512 px PNG. Clay colour, or the textures when an image texture actually loaded.
3. `flux_img2img.py`: flatten the render on a light grey background, VAE encode, FLUX img2img with denoise 0.78 and 22 steps, prompt from the category. The seed is a hash of the id. No ControlNet is installed on spark, so the low denoise is what keeps the silhouette.
4. `filter_iou.py`: the concept mask is the pixels far from the corner background colour (opened, closed, holes filled). IoU against the render alpha at 256 px, best over shifts of up to 6 px. Below `--min-iou` (default 0.75) the image is dropped and written to `dropped.jsonl` with its score and reason.
5. `merge_manifest.py` writes `manifest.synthetic.jsonl`: `has_concept` true, `concept_kind` `synthetic` (existing concepts get `real`), `paths.concept`, `generator`, `prompt`, `concept_silhouette_iou`.

## Ablation

`ablation.py` trains the same small model for the same number of steps and batch size on three data arms and scores it on the real Blender Studio concept pairs, one object held out at a time:

- A: clean renders of the selected objects.
- B: those renders plus their synthetic concepts.
- C: real concepts only (the held-out object removed).

The model is a linear regressor from a 24x24 silhouette and shading feature vector to the object's bounding box proportions and part count. It is a cheap proxy for the question, not the gcad inverse-modelling model, which does not exist yet. The same script and manifest fields drive the real fine-tune once there is one. With 6 real pairs the differences are not significant; more real concept sheets (Spring, Sintel via Blender Studio subscription) are needed.
