# Synthetic concept images (#553)

Gives meshes with no concept art a FLUX concept image, filters the ones that drifted from the shape, and writes them into the manifest.

```
python3 pick.py --per-source 40 --out selection.jsonl
python3 render_all.py selection.jsonl
python3 flux_generate.py selection.jsonl
python3 filter_iou.py --min-iou 0.75
python3 merge_manifest.py
python3 ablation.py
python3 -m unittest
```

Needs Blender (`~/gcad-datasets/tools`), numpy, scipy, Pillow, and ssh access to `spark`, where ComfyUI serves flux1-dev on port 8188. Renders run under `systemd-run -p MemoryMax=6G`: a few GLBs make Blender allocate tens of GB.

## Steps

1. `pick.py`: commercial_ok, gcad_fit high or medium, no concept yet, 50 to 400000 triangles, a fixed number per source, shuffled with a fixed seed.
2. `render_clay.py` (run by `render_all.py`): Blender Workbench, orthographic, azimuth 35 and elevation 25 degrees, transparent 512 px PNG. Clay colour, or the textures when an image texture actually loaded.
3. `flux_generate.py`: flatten the render on a light grey background, VAE encode, FLUX img2img with denoise 0.78 and 22 steps, prompt from the category. The seed is a hash of the id. No ControlNet is installed on spark, so the low denoise is what keeps the silhouette.
4. `filter_iou.py`: the concept mask is the pixels far from the corner background colour (opened, closed, holes filled). IoU against the render alpha at 256 px, best over shifts of up to 6 px. Below `--min-iou` (default 0.75) the image is dropped and written to `dropped.jsonl` with its score and reason.
5. `merge_manifest.py` writes `manifest.synthetic.jsonl`: `has_concept` true, `concept_kind` `synthetic` (existing concepts get `real`), `paths.concept`, `generator`, `prompt`, `concept_silhouette_iou`.

## Ablation

`ablation.py` trains the same small model for the same number of steps and batch size on three data arms and scores it on the real Blender Studio concept pairs, one object held out at a time:

- A: clean renders of the selected objects.
- B: those renders plus their synthetic concepts.
- C: real concepts only (the held-out object removed).

The model is a linear regressor from a 24x24 silhouette and shading feature vector to the object's bounding box proportions and part count. It is a cheap proxy for the question, not the gcad inverse-modelling model, which does not exist yet. The same script and manifest fields drive the real fine-tune once there is one. With 6 real pairs the differences are not significant; more real concept sheets (Spring, Sintel via Blender Studio subscription) are needed.

## Generator comparison (43 objects)

`compare_generators.py` scores both generators and writes a 12 object sheet (render, img2img, ControlNet).

| generator | mean IoU | median IoU | p10 IoU | kept at 0.75 |
| --- | --- | --- | --- | --- |
| FLUX img2img, denoise 0.78 | 0.846 | 0.948 | 0.534 | 38 |
| FLUX + Union-Pro-2.0 canny ControlNet, strength 0.7 | 0.698 | 0.737 | 0.388 | 20 |

Reading the sheet: ControlNet gives clearly more concept-like images (colour, lighting, stylised materials, a chair that is orange instead of grey), but it redraws shape: a gas cylinder turns into a glowing lantern, a gear gets a red core, a bowl gains a ring. img2img stays close to the render but is mostly grey and reads as a re-rendered clay model. The default is img2img because the filter exists to keep shape-faithful pairs, and it keeps almost twice as many. ControlNet is selectable with `--mode controlnet` (model `flux-union-pro2.safetensors` in spark's ComfyUI `models/controlnet`); a lower strength or a colour hint in the prompt is the next thing to try.

## Scale

`selection500.jsonl` holds 498 objects (71 per source, commercial_ok, high or medium fit, no concept). All are rendered. Generation runs on spark at about 50 s per image, so about 7 hours, in the background with `flux_generate.py` (it resumes, skipping finished images). When it ends run `filter_iou.py --meta generated_img2img.jsonl --kept kept_img2img.jsonl --dropped dropped_img2img.jsonl` and `merge_manifest.py --kept kept_img2img.jsonl`. The full 137k run is out of scope: at 50 s per image it is about 80 days of spark time.

## Results (2026-11-12, 43 objects)

43 objects, 6 per source. img2img kept 38 and dropped 5: three near-empty Objaverse renders, the blank Blender Studio default cube rig, and one Zero-to-CAD part at 0.473. Drops are in `dropped_img2img.jsonl`. The merged manifest is `out/manifest.synthetic.jsonl` (38 synthetic, 6 real).

Ablation: leave-one-object-out over the 12 real Blender Studio concept images, 2000 steps, batch 16, 5 seeds, mean absolute error on log bounding box ratios and part count. Arms: A clean renders (38 images), B renders plus synthetic concepts (76), C real concepts only (8 to 11).

| features | A | B | C |
| --- | --- | --- | --- |
| 24x24 silhouette and shading (`--features outline`) | 0.977 | 0.994 | 0.671 |
| frozen DINOv2-small embeddings, CPU (`--features dino`) | 0.760 | 0.734 | 0.617 |

The outline features see only the silhouette, which img2img preserves by design, so B cannot beat A there. With DINOv2 features B improves on A by 0.026 (3%), the direction the idea predicts, but real concepts alone still win by a wider margin, and 6 real objects cannot separate gaps this small. DINO runs need `HF_HUB_OFFLINE=1` once the model is cached and a venv with torch and transformers. A proxy regressor is not the inverse-modelling model: rerun `ablation.py` with that model once it exists.

## Results (500 objects)

Selection of 498 objects, img2img generator. 83 kept and 10 dropped at IoU 0.75 (drops in `dropped_img2img.jsonl`). Arm B here uses 83 synthetic objects.

DINOv2-small features, leave-one-object-out over 12 real concept images, 2000 steps, batch 16, 5 seeds. A is clean renders only, B is renders plus synthetic concepts, C is real concepts only.

| arm | MAE |
| --- | --- |
| A | 0.728 |
| B | 0.760 |
| C | 0.617 |
