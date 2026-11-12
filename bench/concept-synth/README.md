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

## Results (2026-11-12)

Run on 43 objects (6 per source, commercial_ok, high or medium fit). FLUX img2img on spark took about 50 s per image. 38 kept and 5 dropped at IoU 0.75: three Objaverse renders that are near empty, the Blender Studio default cube rig (blank render), and one Zero-to-CAD part at 0.473. The drops are in `~/gcad-datasets/out/synthetic/dropped.jsonl`. The merged manifest is `out/manifest.synthetic.jsonl` (38 synthetic, 6 real; the full 137k manifest is untouched).

Ablation (`ablation.json`, mean absolute error of the log bounding box ratios and part count, 2000 steps, batch 16, 5 seeds, leave-one-object-out over the 12 real Blender Studio concept images):

| arm | train images | MAE |
| --- | --- | --- |
| A clean renders | 38 | 0.977 |
| B renders plus synthetic concepts | 76 | 0.994 |
| C real concepts only | 8 to 11 | 0.671 |

On this proxy real concepts win and synthetic concepts add nothing over clean renders, because the features are silhouette based and FLUX img2img keeps the silhouette by design. It says nothing yet about the full inverse-modelling model, and 6 real objects cannot separate arms with small gaps.
