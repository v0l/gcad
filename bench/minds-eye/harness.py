import datetime
import glob
import json
import os
import random
import shutil
import subprocess
import tempfile
import time
import urllib.request

import agent_api
import judge
from metrics import measure

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
TIERS = {"low": "code", "medium": "code-high", "high": "code-x-high"}
DEFAULT_BUDGET = {"seconds": 900, "tokens": 3_000_000, "turns": 60}

BASE_RULES = """You are modelling in gcad. Your work directory is the current directory. Write the final model to model.gcad there (one operation per line, no comments). Run `gcad check model.gcad` until it passes. Stop when you are happy with it and say done."""
RENDER_RULES = """Look at your model as you go: after every few lines run `gcad render model.gcad view.png` and open it with show_image, then fix what looks wrong (proportions, missing parts, floating pieces). Render and look at least four times before you finish."""
PLAN_RULES = """Before writing any detail, write plan.md: a part list with a size (in the units you will use) and position for every part, and the key proportions. Then block out the parts first as simple shapes, check proportions, and only then add detail."""
REFERENCE_RULES = """A concept image of the target is in reference.png. Model to match it. After you render from a similar view, run the compare tool with your render PNG to get the silhouette overlap with the concept (higher is better, aim above 0.7) and fix the largest differences."""


def load_tasks(sets):
    tasks = []
    for s in sets:
        for ln in open(os.path.join(HERE, "tasks", f"{s}.jsonl")):
            if ln.strip():
                tasks.append(json.loads(ln))
    return tasks


def load_config(path):
    cfg = json.load(open(path))
    cfg.setdefault("name", os.path.splitext(os.path.basename(path))[0])
    cfg.setdefault("agent", "api")
    cfg.setdefault("tier", "medium")
    cfg.setdefault("skill", os.path.join(REPO, "skills", "gcad"))
    cfg.setdefault("render", False)
    cfg.setdefault("reference", False)
    cfg.setdefault("plan", False)
    cfg["budget"] = {**DEFAULT_BUDGET, **cfg.get("budget", {})}
    cfg.setdefault("model", TIERS.get(cfg["tier"], cfg["tier"]))
    return cfg


def system_prompt(cfg):
    skill = open(os.path.join(cfg["skill"], "SKILL.md")).read()
    rules = [BASE_RULES]
    if cfg["render"]:
        rules.append(RENDER_RULES)
    if cfg["plan"]:
        rules.append(PLAN_RULES)
    if cfg["reference"]:
        rules.append(REFERENCE_RULES)
    return "\n\n".join(rules) + "\n\n# Skill\n\n" + skill


def user_prompt(task):
    return f"Request: {task['prompt']}\n\nWrite model.gcad."


def run_agent_api(cfg, task, workdir, log):
    shutil.copytree(cfg["skill"], os.path.join(workdir, "skill"), dirs_exist_ok=True)
    return agent_api.run_api(cfg, user_prompt(task), system_prompt(cfg), workdir, cfg["budget"], log)


def run_agent_cli(cfg, task, workdir, log):
    shutil.copytree(cfg["skill"], os.path.join(workdir, "skill"), dirs_exist_ok=True)
    prompt = system_prompt(cfg) + "\n\n" + user_prompt(task)
    cmd = [a.replace("{prompt}", prompt) for a in cfg["command"]]
    started = time.time()
    try:
        p = subprocess.run(cmd, cwd=workdir, capture_output=True, text=True, timeout=cfg["budget"]["seconds"])
        log.write(json.dumps({"stdout": p.stdout[-4000:], "stderr": p.stderr[-2000:]}) + "\n")
        stop = "finished" if p.returncode == 0 else f"exit {p.returncode}"
    except subprocess.TimeoutExpired:
        stop = "time budget"
    return {"stop": stop, "seconds": round(time.time() - started, 1), "tokens": {}, "tool_calls": None, "renders": None}


def bench_api(path, body=None, method=None):
    url = os.environ.get("AGENTBENCH_URL", "http://127.0.0.1:7420") + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data, {"Authorization": "Bearer " + os.environ["AGENTBENCH_TOKEN"],
                                              "Content-Type": "application/json"}, method=method)
    raw = urllib.request.urlopen(req, timeout=60).read()
    return json.loads(raw) if raw else None


def run_agent_agentbench(cfg, task, workdir, log):
    branch = f"minds-eye-{task['id']}-{int(time.time())}"
    shutil.copytree(cfg["skill"], os.path.join(workdir, "skill"), dirs_exist_ok=True)
    prompt = (BASE_RULES + "\n\n" + (RENDER_RULES if cfg["render"] else "") + "\n\n" + (PLAN_RULES if cfg["plan"] else "")
              + f"\n\nRead the skill at {cfg['skill']}/SKILL.md first.\n\n" + user_prompt(task))
    s = bench_api("/api/sessions", {"project_id": int(os.environ.get("AGENTBENCH_PROJECT", 6)), "agent": "native",
                                    "model": cfg["tier"], "title": f"minds-eye {task['id']}",
                                    "place": {"kind": "new", "branch": branch}, "prompt": prompt})
    sid = s["id"]
    started = time.time()
    stop = "finished"
    while True:
        cur = bench_api(f"/api/sessions/{sid}")
        if cur["status"] in ("idle", "failed", "stopped"):
            break
        if time.time() - started > cfg["budget"]["seconds"]:
            bench_api(f"/api/sessions/{sid}/interrupt", {})
            stop = "time budget"
            break
        time.sleep(15)
    cwd = cur["cwd"]
    model = os.path.join(cwd, "model.gcad")
    if os.path.exists(model):
        shutil.copy(model, os.path.join(workdir, "model.gcad"))
    u = cur["usage"]
    msgs = bench_api(f"/api/sessions/{sid}/messages?after=0&limit=5000")
    calls = sum(1 for m in msgs if m["entry"].get("kind") == "tool")
    log.write(json.dumps({"session": sid, "cwd": cwd}) + "\n")
    return {"stop": stop, "seconds": round(time.time() - started, 1),
            "tokens": {"input": u["input_tokens"], "output": u["output_tokens"], "cached": u["cache_read_tokens"],
                       "requests": u["turns"]},
            "tool_calls": calls, "renders": None}


AGENTS = {"api": run_agent_api, "cli": run_agent_cli, "agentbench": run_agent_agentbench}


def run_one(cfg, task, seed, keep):
    workdir = tempfile.mkdtemp(prefix=f"me-{task['id']}-")
    if cfg["reference"]:
        import concept

        shutil.copy(concept.concept(task, seed), os.path.join(workdir, "reference.png"))
    runlog = os.path.join(keep, "agent.jsonl")
    os.makedirs(keep, exist_ok=True)
    with open(runlog, "w") as log:
        try:
            run = AGENTS[cfg["agent"]](cfg, task, workdir, log)
        except Exception as e:
            run = {"stop": f"error: {e}", "seconds": 0, "tokens": {}, "tool_calls": 0, "renders": 0}
    model = os.path.join(workdir, "model.gcad")
    hard = measure(model, task, workdir)
    if os.path.exists(model):
        shutil.copy(model, os.path.join(keep, "model.gcad"))
    views = judge.render_views(model, keep) if hard["builds"] else None
    scored = None
    if views and not os.environ.get("NO_JUDGE"):
        try:
            scored = judge.score(task, views)
        except Exception as e:
            scored = {"error": str(e)}
    shutil.rmtree(workdir, ignore_errors=True)
    return {"task": task["id"], "domain": task["domain"], "seed": seed, "run": run, "hard": hard,
            "judge": scored, "views": [os.path.relpath(v, RESULTS) for v in views] if views else None,
            "model": os.path.relpath(os.path.join(keep, "model.gcad"), RESULTS)}


def summarize(rows):
    def mean(xs):
        xs = [x for x in xs if x is not None]
        return round(sum(xs) / len(xs), 3) if xs else None

    judged = [r["judge"] for r in rows if r["judge"] and "overall" in r["judge"]]
    by_task = {}
    for r in rows:
        if r["judge"] and "overall" in r["judge"]:
            by_task.setdefault(r["task"], []).append(r["judge"]["overall"])
    spread = [max(v) - min(v) for v in by_task.values() if len(v) > 1]
    return {
        "runs": len(rows),
        "build_rate": mean([1.0 if r["hard"]["builds"] else 0.0 for r in rows]),
        "judge_overall": mean([j["overall"] for j in judged]),
        "rubric_fraction": mean([j["rubric_fraction"] for j in judged]),
        "hard_score": mean([r["hard"]["hard_score"] for r in rows]),
        "triangles": mean([r["hard"].get("triangles") for r in rows]),
        "named_parts": mean([r["hard"].get("named_parts") for r in rows]),
        "silhouette_complexity": mean([r["hard"].get("silhouette_complexity_mean") for r in rows]),
        "mesh_ops_used": mean([1.0 if r["hard"].get("beyond_primitives") else 0.0 for r in rows
                              ]),
        "tokens": mean([sum((r["run"]["tokens"] or {}).get(k, 0) for k in ("input", "output")) for r in rows]),
        "seconds": mean([r["run"]["seconds"] for r in rows]),
        "tool_calls": mean([r["run"]["tool_calls"] for r in rows]),
        "seed_spread": mean(spread),
    }


def cmd_run(args):
    cfg = load_config(args.config)
    tasks = load_tasks(args.sets)
    if args.tasks:
        tasks = [t for t in tasks if t["id"] in args.tasks]
    if args.limit:
        tasks = tasks[: args.limit]
    day = datetime.date.today().isoformat()
    stamp = f"{day}-{cfg['name']}"
    rows = []
    for t in tasks:
        for seed in range(args.seeds):
            keep = os.path.join(RESULTS, "runs", stamp, f"{t['id']}-s{seed}")
            print(f"{cfg['name']} {t['id']} seed {seed}", flush=True)
            row = run_one(cfg, t, seed, keep)
            rows.append(row)
            j = row["judge"] or {}
            print(f"  builds={row['hard']['builds']} hard={row['hard']['hard_score']} judge={j.get('overall')} "
                  f"stop={row['run']['stop']} {row['run']['seconds']}s", flush=True)
            save(stamp, cfg, rows, args.sets)
    print(json.dumps(summarize(rows), indent=1))


def save(stamp, cfg, rows, sets):
    os.makedirs(RESULTS, exist_ok=True)
    out = {"config": cfg, "sets": sets, "date": stamp[:10], "judge_version": judge.JUDGE_VERSION,
           "judge_model": judge.JUDGE_MODEL, "summary": summarize(rows), "rows": rows}
    json.dump(out, open(os.path.join(RESULTS, f"{stamp}.json"), "w"), indent=1)


def load_result(path):
    return json.load(open(path))


def cmd_pairwise(args):
    a, b = load_result(args.a), load_result(args.b)
    rows_b = {(r["task"], r["seed"]): r for r in b["rows"]}
    tasks = {t["id"]: t for t in load_tasks(["tuning", "heldout"])}
    rng = random.Random(args.rng)
    votes = []
    for r in a["rows"]:
        o = rows_b.get((r["task"], r["seed"]))
        if not o or not r["views"] or not o["views"]:
            continue
        flip = rng.random() < 0.5
        va = [os.path.join(RESULTS, v) for v in r["views"]]
        vb = [os.path.join(RESULTS, v) for v in o["views"]]
        v = judge.blind_pair(tasks[r["task"]], va, vb, flip)
        votes.append({"task": r["task"], "seed": r["seed"], "flip": flip, **v})
        print(r["task"], v["winner"], flush=True)
    wins = sum(v["winner"] == "x" for v in votes)
    losses = sum(v["winner"] == "y" for v in votes)
    ties = len(votes) - wins - losses
    out = {"a": a["config"]["name"], "b": b["config"]["name"], "wins_a": wins, "wins_b": losses, "ties": ties,
           "win_rate_a": round((wins + ties / 2) / len(votes), 3) if votes else None, "votes": votes}
    name = f"{datetime.date.today().isoformat()}-pair-{out['a']}-vs-{out['b']}.json"
    json.dump(out, open(os.path.join(RESULTS, name), "w"), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != "votes"}, indent=1))


def cmd_agreement(args):
    ratings = json.load(open(args.ratings))
    paired = []
    for name, human in ratings.items():
        task_id = name.split("/")[0].rsplit("-s", 1)[0]
        task = {t["id"]: t for t in load_tasks(["tuning", "heldout"])}[task_id]
        folder = os.path.join(RESULTS, "runs", name)
        views = [os.path.join(folder, "views4.png"), os.path.join(folder, "views6.png")]
        j = judge.score(task, views)
        paired.append((human, j["overall"]))
        print(name, human, j["overall"], flush=True)
    n = len(paired)
    h = [p[0] for p in paired]
    m = [p[1] for p in paired]
    mean_abs = sum(abs(a - b) for a, b in paired) / n
    within1 = sum(abs(a - b) <= 1 for a, b in paired) / n
    out = {"n": n, "pearson": pearson(h, m), "spearman": pearson(ranks(h), ranks(m)), "mean_abs_error": round(mean_abs, 2),
           "within_1": round(within1, 2), "judge": f"{judge.JUDGE_VERSION}:{judge.JUDGE_MODEL}"}
    json.dump(out, open(os.path.join(RESULTS, f"{datetime.date.today().isoformat()}-agreement.json"), "w"), indent=1)
    print(json.dumps(out, indent=1))


def ranks(xs):
    order = sorted(range(len(xs)), key=lambda i: xs[i])
    r = [0.0] * len(xs)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and xs[order[j + 1]] == xs[order[i]]:
            j += 1
        for k in range(i, j + 1):
            r[order[k]] = (i + j) / 2 + 1
        i = j + 1
    return r


def pearson(a, b):
    n = len(a)
    ma, mb = sum(a) / n, sum(b) / n
    sa = sum((x - ma) ** 2 for x in a) ** 0.5
    sb = sum((y - mb) ** 2 for y in b) ** 0.5
    if not sa or not sb:
        return None
    return round(sum((x - ma) * (y - mb) for x, y in zip(a, b)) / (sa * sb), 3)


def cmd_rating_sheet(args):
    import judge as j

    runs = sorted(glob.glob(os.path.join(RESULTS, "runs", "*", "*", "views4.png")))
    random.Random(args.rng).shuffle(runs)
    pick = runs[: args.n]
    names = [os.path.relpath(os.path.dirname(p), os.path.join(RESULTS, "runs")) for p in pick]
    json.dump({n: None for n in names}, open(args.out, "w"), indent=1)
    print(f"wrote {args.out}: fill in a 1-10 rating for each run, then run `run agreement {args.out}`")
    for n in names:
        print(n)


def main():
    import argparse

    ap = argparse.ArgumentParser(prog="run")
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("config")
    r.add_argument("--sets", nargs="+", default=["tuning"])
    r.add_argument("--tasks", nargs="*")
    r.add_argument("--limit", type=int)
    r.add_argument("--seeds", type=int, default=2)
    r.set_defaults(fn=cmd_run)
    p = sub.add_parser("pairwise")
    p.add_argument("a")
    p.add_argument("b")
    p.add_argument("--rng", type=int, default=7)
    p.set_defaults(fn=cmd_pairwise)
    g = sub.add_parser("agreement")
    g.add_argument("ratings")
    g.set_defaults(fn=cmd_agreement)
    s = sub.add_parser("rating-sheet")
    s.add_argument("--n", type=int, default=20)
    s.add_argument("--out", default="ratings.json")
    s.add_argument("--rng", type=int, default=3)
    s.set_defaults(fn=cmd_rating_sheet)
    rp = sub.add_parser("report")
    rp.add_argument("results", nargs="+")
    rp.add_argument("--out", default=None)
    rp.set_defaults(fn=lambda a: __import__("report").build(a.results, a.out))
    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
