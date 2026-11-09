import base64
import html
import json
import os

from harness import RESULTS

JUDGE_NOTE = "Scores: judge is the vision model overall 1-10; rubric is the fraction of rubric items present; hard is the 0-10 score from the built model."


def thumb(rel):
    from PIL import Image
    import io

    im = Image.open(os.path.join(RESULTS, rel)).convert("RGB")
    im.thumbnail((520, 520))
    buf = io.BytesIO()
    im.save(buf, "JPEG", quality=80)
    return "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode()


def cell(row):
    if not row:
        return "<td class=none>no run</td>"
    j = row.get("judge") or {}
    h = row["hard"]
    img = f"<img src='{thumb(row['views'][0])}'>" if row.get("views") else "<div class=fail>" + html.escape(h.get("build_error") or "no build") + "</div>"
    return (f"<td>{img}<div class=s>judge {j.get('overall', '-')} | rubric {j.get('rubric_fraction', '-')} | hard {h.get('hard_score')}"
            f"<br>tris {h.get('triangles', '-')} parts {h.get('named_parts', '-')} {row['run']['seconds']}s</div></td>")


def summary_table(results):
    keys = ["runs", "build_rate", "judge_overall", "rubric_fraction", "hard_score", "triangles", "named_parts",
            "silhouette_complexity", "mesh_ops_used", "tokens", "seconds", "tool_calls", "seed_spread"]
    head = "".join(f"<th>{k}</th>" for k in keys)
    body = ""
    for r in results:
        s = r["summary"]
        body += f"<tr><th>{html.escape(r['config']['name'])}</th>" + "".join(f"<td>{s.get(k)}</td>" for k in keys) + "</tr>"
    return f"<table><tr><th>config</th>{head}</tr>{body}</table>"


def build(paths, out=None):
    results = [json.load(open(p)) for p in paths]
    tasks = []
    for r in results:
        for row in r["rows"]:
            if row["task"] not in tasks:
                tasks.append(row["task"])
    seeds = sorted({row["seed"] for r in results for row in r["rows"]})
    grid = ""
    for t in tasks:
        grid += f"<h3>{t}</h3><table><tr>" + "".join(f"<th>{html.escape(r['config']['name'])}</th>" for r in results) + "</tr>"
        for s in seeds:
            grid += "<tr>"
            for r in results:
                grid += cell(next((x for x in r["rows"] if x["task"] == t and x["seed"] == s), None))
            grid += "</tr>"
        grid += "</table>"
    page = f"""<!doctype html><meta charset=utf-8><title>Minds-eye report</title>
<style>body{{font:14px sans-serif;margin:20px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:4px;vertical-align:top}}
img{{width:360px}}.s{{font-size:12px;color:#444}}.fail{{width:360px;color:#a00;font-size:12px}}</style>
<h1>Minds-eye report</h1><p>{JUDGE_NOTE}</p>{summary_table(results)}{grid}"""
    out = out or os.path.join(RESULTS, "report.html")
    open(out, "w").write(page)
    print("wrote", out)
