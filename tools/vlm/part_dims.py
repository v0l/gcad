import math
import random
import re
from dataclasses import dataclass, field

POINT = re.compile(r"^-?\d+(?:\.\d+)?,-?\d+(?:\.\d+)?$")
SPEC_NUMBER = re.compile(r"(?<![A-Za-z])\d+(?:\.\d+)?")
IGNORED_OPS = {"plane", "move", "rotate", "axis", "body", "combine", "let", "color", "colour", "material", "name"}
WORDS = ["none", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"]
MAX_FEATURES = 12
TOL = 1e-3


def fmt(x):
    return f"{abs(x):.2f}".rstrip("0").rstrip(".") or "0"


def count_word(n):
    return WORDS[n] if 0 < n < len(WORDS) else "several"


def plural(name):
    return name + ("es" if name.endswith("s") else "s")


def number(token):
    return float(token)


def point(token):
    if token and POINT.match(token):
        x, y = token.split(",")
        return float(x), float(y)
    return 0.0, 0.0


def keyvalues(tokens):
    return {t.split("=", 1)[0]: t.split("=", 1)[1] for t in tokens if "=" in t}


@dataclass
class Profile:
    kind: str
    dims: tuple
    bbox: tuple = None
    radius: float = 0.0
    sides: int = 0
    points: list = field(default_factory=list)

    def signature(self):
        return (self.kind, tuple(round(d, 3) for d in self.dims), round(self.radius, 3), self.sides)

    def size_text(self):
        if self.kind == "circle":
            return f"{fmt(self.dims[0])} mm diameter"
        if self.kind == "ngon":
            return f"{WORDS[self.sides] if self.sides < len(WORDS) else 'many'}-sided, {fmt(self.dims[0])} mm across corners"
        if self.kind == "outline":
            return f"{fmt(self.dims[0])} x {fmt(self.dims[1])} overall"
        return f"{fmt(self.dims[0])} x {fmt(self.dims[1])}"

    def base_lead(self, depth):
        d = fmt(depth)
        if self.kind == "rect":
            w, h = fmt(self.dims[0]), fmt(self.dims[1])
            return [f"rectangular base {w} x {h}, {d} mm thick", f"{w} x {h} base plate, {d} mm thick", f"{w} x {h} x {d} mm block"]
        if self.kind == "circle":
            n = fmt(self.dims[0])
            return [f"round base {n} mm diameter, {d} mm thick", f"{n} mm diameter disc, {d} mm thick", f"cylinder {n} mm diameter, {d} mm long"]
        text = self.size_text()
        return [f"base profile {text}, extruded {d} mm", f"{text} footprint, {d} mm thick"]

    def pocket_name(self):
        return {"rect": "rectangular pocket", "circle": "round pocket", "ngon": "polygonal pocket", "slot": "slot"}.get(self.kind, "profile cut")

    def boss_name(self):
        return {"rect": "rectangular block", "circle": "round boss", "ngon": "polygonal boss", "slot": "slotted block"}.get(self.kind, "profiled block")

    def opening_name(self):
        return {"rect": "rectangular opening", "circle": "round opening", "ngon": "polygonal opening", "slot": "slot opening"}.get(self.kind, "opening")

    def inside(self, other):
        if not self.bbox or not other.bbox:
            return False
        a, b = self.bbox, other.bbox
        return a[0] >= b[0] - TOL and a[1] >= b[1] - TOL and a[2] <= b[2] + TOL and a[3] <= b[3] + TOL and a != b


def bbox_of(points):
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    return min(xs), min(ys), max(xs), max(ys)


def outline(kind, points):
    box = bbox_of(points)
    return Profile(kind, (box[2] - box[0], box[3] - box[1]), bbox=box, points=list(points))


def centred(kind, w, h, at, radius=0.0, sides=0):
    cx, cy = at
    return Profile(kind, (w, h) if kind != "circle" else (w,), bbox=(cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2), radius=radius, sides=sides)


@dataclass
class Feature:
    lead: list
    clauses: list
    main: bool = False

    def key(self):
        return (self.lead[0] if self.lead else "", tuple(c[1][0] for c in self.clauses))


@dataclass
class Spec:
    features: list
    unphrased: list


def edges_text(selector):
    parts = []
    for term in selector.split("|"):
        phrase = single_edges(term)
        if phrase is None:
            return ""
        if phrase not in parts:
            parts.append(phrase)
    return " and ".join(parts)


def single_edges(term):
    if term == "all":
        return "all edges"
    groups = term.split("&")
    if len(groups) == 1 or groups[1] == "all":
        face = groups[0]
        names = {">Z": "top", "<Z": "bottom", ">X": "+X face", "<X": "-X face", ">Y": "+Y face", "<Y": "-Y face"}
        if face in names:
            return f"{names[face]} edges" if face[1] == "Z" else f"edges of the {names[face]}"
        return None
    axes = []
    for group in groups:
        letters = {t[1:] for t in group.split(",") if t[:1] in "+-" and len(t) == 2}
        if len(letters) != 1 or len(group.split(",")) != 2:
            return None
        axes.append(letters.pop())
    if len(set(axes)) == 2 and len(axes) == 2:
        third = ({"X", "Y", "Z"} - set(axes)).pop()
        return "the vertical corner edges" if third == "Z" else f"the edges parallel to {third}"
    return None


def hole_pattern(points):
    if len(points) < 2:
        return None
    xs = sorted({round(p[0], 4) for p in points})
    ys = sorted({round(p[1], 4) for p in points})
    if len(xs) * len(ys) != len(points):
        return None
    if len(points) == 2:
        gap = max(xs[-1] - xs[0], ys[-1] - ys[0])
        return [f"{fmt(gap)} mm apart", f"spaced {fmt(gap)} mm"]
    if len(xs) == 1 or len(ys) == 1:
        line = ys if len(xs) == 1 else xs
        pitches = [b - a for a, b in zip(line, line[1:])]
        if max(pitches) - min(pitches) > 1e-3:
            return None
        return [f"in a row at {fmt(pitches[0])} mm pitch"]
    px = (xs[1] - xs[0]) if len(xs) > 1 else 0
    py = (ys[1] - ys[0]) if len(ys) > 1 else 0
    if any(abs((b - a) - px) > 1e-3 for a, b in zip(xs, xs[1:])) or any(abs((b - a) - py) > 1e-3 for a, b in zip(ys, ys[1:])):
        return None
    if len(xs) == 2 and len(ys) == 2:
        return [f"on a {fmt(px)} x {fmt(py)} pattern", f"{fmt(px)} x {fmt(py)} spacing"]
    return [f"in a {count_word(len(xs))} by {count_word(len(ys))} grid, {fmt(px)} x {fmt(py)} spacing"]


def hole_feature(tokens, kv):
    args = [t for t in tokens[1:] if "=" not in t]
    if not args:
        return None
    pts = [point(t) for t in args[1:] if POINT.match(t)]
    if not pts:
        return None
    lead = [f"{count_word(len(pts))} hole" + ("" if len(pts) == 1 else "s")]
    try:
        diameter = [f"{fmt(number(args[0]))} mm diameter", f"{fmt(number(args[0]))} mm dia"]
    except ValueError:
        diameter = [f"size {args[0]}"]
    clauses = [(True, diameter)]
    if "depth" in kv:
        clauses.append((True, [f"{fmt(number(kv['depth']))} mm deep", f"depth {fmt(number(kv['depth']))}"]))
    else:
        clauses.append((True, ["through", "through all"]))
    if "cbore" in kv:
        a, b = kv["cbore"].split(",")
        clauses.append((True, [f"counterbored {fmt(number(a))} mm wide, {fmt(number(b))} mm deep"]))
    if "csink" in kv:
        a = kv["csink"].split(",")[0]
        clauses.append((True, [f"countersunk {fmt(number(a))} mm"]))
    pattern = hole_pattern(pts)
    if pattern:
        clauses.append((True, pattern))
    return Feature(lead, clauses)


def edge_feature(name, tokens):
    args = [t for t in tokens[1:] if "=" not in t]
    size = fmt(number(args[0]))
    edges = edges_text(args[1]) if len(args) > 1 else ""
    if edges:
        variants = [f"{size} mm {name} on {edges}", f"{edges}: {size} mm {name}"]
    else:
        variants = [f"{size} mm {name}", f"{name} {size} mm"]
    return Feature([""], [(True, variants)])


def grouped(profiles):
    groups = []
    for profile in profiles:
        for group in groups:
            if group[0].signature() == profile.signature():
                group[1] += 1
                break
        else:
            groups.append([profile, 1])
    return groups


def group_feature(profile, count, name, depth_clause):
    word = count_word(count)
    lead = [f"{word} {plural(name) if count > 1 else name}"]
    clauses = [(True, [profile.size_text()])]
    if depth_clause:
        clauses.append((True, depth_clause))
    return Feature(lead, clauses)


class Extractor:
    def __init__(self):
        self.features = []
        self.unphrased = set()
        self.profiles = []
        self.sections = []
        self.path = []
        self.base = None

    def add(self, feature):
        if feature and feature.key() not in {f.key() for f in self.features}:
            self.features.append(feature)

    def line(self, raw):
        tokens = raw.split()
        if not tokens:
            return
        if tokens[0].endswith(":") and len(tokens) > 1:
            tokens = tokens[1:]
        op = tokens[0]
        try:
            self.dispatch(op, tokens)
        except (ValueError, IndexError, KeyError):
            self.unphrased.add(op)

    def dispatch(self, op, tokens):
        kv = keyvalues(tokens)
        args = [t for t in tokens[1:] if "=" not in t]
        if op == "rect":
            w, h = number(args[0]), number(args[1])
            self.profiles.append(centred("rect", w, h, point(kv.get("at")), radius=number(kv.get("r", "0"))))
        elif op == "circle":
            self.profiles.append(centred("circle", number(args[0]), number(args[0]), point(kv.get("at"))))
        elif op == "ngon":
            self.profiles.append(centred("ngon", number(args[0]), number(args[0]), point(kv.get("at")), sides=int(number(args[1]))))
        elif op == "slot":
            length, width = number(args[0]), number(args[1])
            angle = number(kv.get("angle", "0"))
            profile = centred("slot", length, width, point(kv.get("at")))
            if angle % 180 != 0:
                profile.bbox = None
            self.profiles.append(profile)
        elif op == "poly":
            self.profiles.append(outline("outline", [point(t) for t in args if POINT.match(t)]))
        elif op == "pen":
            self.path = [point(args[0])]
        elif op == "line":
            self.path.append(point(args[0]))
        elif op == "arc":
            self.path.append(point(kv["via"]))
            self.path.append(point(args[0]))
        elif op == "close":
            if len(self.path) >= 3:
                self.profiles.append(outline("outline", self.path))
            self.path = []
        elif op == "section":
            self.sections.append(self.profiles[0] if self.profiles else None)
            self.profiles = []
        elif op == "loft":
            self.loft()
        elif op == "extrude":
            self.extrude(tokens, kv, args)
        elif op == "revolve":
            self.revolve(kv)
        elif op == "hole":
            self.add(hole_feature(tokens, kv))
        elif op in ("fillet", "chamfer"):
            self.add(edge_feature(op, tokens))
        elif op == "shell":
            t = fmt(number(args[0]))
            self.add(Feature([""], [(True, [f"walls {t} mm thick", f"{t} mm wall thickness", f"shelled to {t} mm"])]))
        elif op not in IGNORED_OPS:
            self.unphrased.add(op)

    def loft(self):
        shapes = [s for s in self.sections if s]
        self.sections = []
        if len(shapes) < 2:
            self.unphrased.add("loft")
            return
        a, b = shapes[0], shapes[-1]
        noun = lambda p: {"rect": "rectangle", "circle": "circle"}.get(p.kind, "profile")
        text = f"lofted between a {a.size_text()} {noun(a)} and a {b.size_text()} {noun(b)}"
        self.add(Feature([text], [], main=True))

    def revolve(self, kv):
        profile = self.profiles[-1] if self.profiles else None
        self.profiles = []
        if not profile or not profile.points:
            self.unphrased.add("revolve")
            return
        along = 0 if kv.get("axis", "y") == "x" else 1
        across = 1 - along
        radius = max(abs(p[across]) for p in profile.points)
        values = [p[along] for p in profile.points]
        length = max(values) - min(values)
        d, h = fmt(2 * radius), fmt(length)
        self.add(Feature([f"turned part, {d} mm maximum diameter and {h} mm long", f"revolved profile, {d} mm largest diameter, {h} mm tall"], [], main=True))

    def extrude(self, tokens, kv, args):
        depth = abs(number(args[0]))
        profiles, self.profiles = self.profiles, []
        if not profiles:
            self.unphrased.add("extrude")
            return
        cut = kv.get("mode") == "cut"
        groups = grouped(profiles)
        first_is_base = self.base is None and not cut
        if first_is_base:
            self.base = groups[0][0]
            base = Feature(self.base.base_lead(depth), [], main=True)
            if self.base.kind == "rect" and self.base.radius > 0:
                r = fmt(self.base.radius)
                base.clauses.append((True, [f"{r} mm corner radius", f"corners rounded to {r} mm"]))
            self.add(base)
            groups = groups[1:]
        for profile, count in groups:
            depth_variants = [f"{fmt(depth)} mm deep", f"depth {fmt(depth)}"] if cut else [f"{fmt(depth)} mm tall", f"extruded {fmt(depth)} mm"]
            if cut:
                self.add(group_feature(profile, count, profile.pocket_name(), depth_variants))
            elif self.base is not None and profile.inside(self.base) and first_is_base:
                self.add(group_feature(profile, count, profile.opening_name(), None))
            else:
                self.add(group_feature(profile, count, profile.boss_name(), None if first_is_base else depth_variants))


def extract(program):
    ex = Extractor()
    for raw in program.splitlines():
        ex.line(raw)
    features = ex.features
    main = [f for f in features if f.main]
    minor = [f for f in features if not f.main][: max(0, MAX_FEATURES - len(main))]
    keep = set(map(id, main + minor))
    return Spec([f for f in features if id(f) in keep], sorted(ex.unphrased))


def render(spec, rng, p=0.3):
    phrases = []
    for feature in spec.features:
        parts = [rng.choice(feature.lead)] if feature.lead and feature.lead[0] else []
        kept = 0
        for minor, variants in feature.clauses:
            dropped = minor and rng.random() < p
            text = rng.choice(variants)
            if not dropped:
                parts.append(text)
                kept += 1
        if feature.main or kept:
            phrases.append(", ".join(parts))
    if not phrases:
        return ""
    if rng.random() < 0.3:
        return "\n" + "\n".join(f"- {t}" for t in phrases)
    return "; ".join(phrases) + "."


def describe(program, key, p=0.3, seed=1):
    return render(extract(program), random.Random(f"{seed}:{key}"), p)


def program_numbers(program):
    values = []
    for raw in program.splitlines():
        for token in re.split(r"[\s,=]+", raw):
            try:
                values.append(float(token))
            except ValueError:
                pass
    return values


def allowed_numbers(program):
    values = sorted(set(program_numbers(program)))[:300]
    allowed = set()
    for a in values:
        allowed.add(abs(a))
        allowed.add(abs(2 * a))
        for b in values:
            allowed.add(abs(a - b))
    return allowed


def spec_numbers(spec_text):
    return [float(m) for m in SPEC_NUMBER.findall(spec_text)]


def invented_numbers(program, spec_text):
    allowed = allowed_numbers(program)
    return [n for n in spec_numbers(spec_text) if not any(abs(n - a) <= 0.0051 for a in allowed)]


def main():
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("gcad")
    parser.add_argument("--p", type=float, default=0.3)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()
    program = open(args.gcad).read()
    spec = extract(program)
    print(render(spec, random.Random(f"{args.seed}:{args.gcad}"), args.p))
    print("unphrased:", ", ".join(spec.unphrased) or "none")


if __name__ == "__main__":
    main()
