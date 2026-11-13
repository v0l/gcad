import contextlib
import io
import math
import signal
import sys
import types

import cadquery as real_cq
from cadquery import Location, Vector


class Unsupported(AttributeError):
    pass


def num(x):
    s = format(round(float(x), 5), ".5f").rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def pt(p):
    return f"{num(p[0])},{num(p[1])}"


def pt3(v):
    return f"{num(v.x)},{num(v.y)},{num(v.z)}"


def as_flags(c):
    return (c, c, c) if isinstance(c, bool) else tuple(c)


PASS = {
    "center", "pushPoints", "rarray", "polarArray", "transformed", "moveTo", "move",
    "newObject", "tag", "wire",
}
VALUES = {"val", "vals", "size", "findSolid", "plane", "objects", "ctx", "solids", "largestDimension"}
SEGMENTS = {"lineTo", "line", "hLine", "vLine", "threePointArc", "radiusArc", "sagittaArc", "polyline", "tangentArcPoint"}
EDGE_TERMS = {
    "|Z": "+X,-X&+Y,-Y",
    "|X": "+Y,-Y&+Z,-Z",
    "|Y": "+X,-X&+Z,-Z",
    ">Z": ">Z&all",
    "<Z": "<Z&all",
    ">X": ">X&all",
    "<X": "<X&all",
    ">Y": ">Y&all",
    "<Y": "<Y&all",
    "<<Z": "<Z&all",
    ">>Z": ">Z&all",
    "<<X": "<X&all",
    ">>X": ">X&all",
    "<<Y": "<Y&all",
    ">>Y": ">Y&all",
    "#Z": ">Z&all|<Z&all",
    "not |Z": ">Z&all|<Z&all",
    "(not |Z)": ">Z&all|<Z&all",
}
AND_EDGES = {
    ("|Z", ">X"): ">X&+Y,-Y",
    ("|Z", "<X"): "<X&+Y,-Y",
    ("|Z", ">Y"): ">Y&+X,-X",
    ("|Z", "<Y"): "<Y&+X,-X",
    ("|X", ">Y"): ">Y&+Z,-Z",
    ("|X", "<Y"): "<Y&+Z,-Z",
    ("|X", ">Z"): ">Z&+Y,-Y",
    ("|X", "<Z"): "<Z&+Y,-Y",
    ("|Y", ">X"): ">X&+Z,-Z",
    ("|Y", "<X"): "<X&+Z,-Z",
    ("|Y", ">Z"): ">Z&+X,-X",
    ("|Y", "<Z"): "<Z&+X,-X",
}
FACE_SELECTORS = {">Z", "<Z", ">X", "<X", ">Y", "<Y", "+Z", "-Z", "+X", "-X", "+Y", "-Y"}


PARALLEL = {"|X", "|Y", "|Z"}
EXTREME_FACES = {">X", "<X", ">Y", "<Y", ">Z", "<Z"}


def edge_selector_of(text):
    terms = []
    for part in text.split(" or "):
        pieces = [p.strip() for p in part.split(" and ")]
        if len(pieces) == 1 and pieces[0] in EDGE_TERMS:
            terms.append(EDGE_TERMS[pieces[0]])
        elif len(pieces) == 2 and tuple(pieces) in AND_EDGES:
            terms.append(AND_EDGES[tuple(pieces)])
        elif len(pieces) == 2 and all(p in EXTREME_FACES for p in pieces):
            terms.append("&".join(pieces))
        elif len(pieces) == 3 and pieces[0] in PARALLEL and all(p in EXTREME_FACES for p in pieces[1:]):
            terms.append("&".join(pieces[1:]))
        else:
            raise Unsupported("edges_selector")
    return "|".join(terms)


def face_selector_of(text):
    pieces = [p.strip() for p in text.split(" or ")]
    if any(p not in FACE_SELECTORS for p in pieces):
        raise Unsupported("faces_selector")
    return ",".join(pieces)


SKETCH_HEADS = {"rect", "circle", "poly", "ngon", "slot", "ellipse", "pen", "line", "arc", "close", "plane", "spline"}
XY_KEY = (0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0)


class W:
    def __init__(self, *args, **kwargs):
        if "obj" in kwargs:
            raise Unsupported("Workplane(obj)")
        self.real = real_cq.Workplane(*args, **kwargs)
        self.lines = []
        self.pkey = XY_KEY
        self.shifted = False
        self.sel = None
        self.pen = False

    @classmethod
    def make(cls, real, parent, add=(), **change):
        w = object.__new__(cls)
        w.real = real
        w.lines = list(change.get("lines", parent.lines)) + list(add)
        w.pkey = change.get("plane", parent.pkey)
        w.sel = change.get("sel", None)
        w.pen = change.get("pen", parent.pen)
        w.shifted = change.get("shifted", False)
        return w

    def __getattr__(self, name):
        if name.startswith("__"):
            raise AttributeError(name)
        handler = getattr(type(self), "op_" + name, None)
        if handler is not None:
            return handler.__get__(self)
        if name in PASS:
            return lambda *a, **k: W.make(getattr(self.real, name)(*a, **k), self)
        if name in VALUES:
            return getattr(self.real, name)
        raise Unsupported(name)

    def plane_lines(self):
        if self.shifted:
            return [], None
        p = self.real.plane
        key = tuple(round(v, 6) for v in (*p.origin.toTuple(), *p.xDir.toTuple(), *p.zDir.toTuple()))
        if key == self.pkey:
            return [], key
        y = p.zDir.cross(p.xDir)
        pts = [p.origin, p.origin.add(p.xDir), p.origin.add(y)]
        return ["plane " + " ".join(pt3(v) for v in pts)], key

    def points(self):
        p = self.real.plane
        objs = self.real.objects
        if not objs:
            return [(0.0, 0.0, 0.0)]
        out = []
        for o in objs:
            if isinstance(o, Location):
                pos, rot = o.toTuple()
                out.append((pos[0], pos[1], rot[2]))
                continue
            if isinstance(o, Vector):
                v = o
            elif hasattr(o, "Center"):
                v = o.Center()
            else:
                raise Unsupported("stack_object")
            local = p.toLocalCoords(v)
            out.append((local.x, local.y, 0.0))
        return out

    def emit(self, lines, **change):
        pl, key = self.plane_lines()
        change.setdefault("plane", key)
        return pl + lines, change

    def drawn(self, real, lines):
        out, change = self.emit(lines)
        return W.make(real, self, out, **change)

    def op_rect(self, xLen, yLen, centered=True, forConstruction=False):
        real = self.real.rect(xLen, yLen, centered, forConstruction)
        if forConstruction:
            return W.make(real, self)
        return self.drawn(real, self.rect_lines(xLen, yLen, centered))

    def rect_lines(self, xLen, yLen, centered):
        cx, cy = as_flags(centered)[:2]
        lines = []
        for x, y, rot in self.points():
            ox = 0 if cx else xLen / 2
            oy = 0 if cy else yLen / 2
            if abs(math.remainder(rot, 360)) < 1e-6:
                lines.append(f"rect {num(xLen)} {num(yLen)} at={pt((x + ox, y + oy))}")
                continue
            a = math.radians(rot)
            corners = [(-xLen / 2, -yLen / 2), (xLen / 2, -yLen / 2), (xLen / 2, yLen / 2), (-xLen / 2, yLen / 2)]
            if not (cx and cy):
                corners = [(u + ox, v + oy) for u, v in corners]
            moved = [(x + u * math.cos(a) - v * math.sin(a), y + u * math.sin(a) + v * math.cos(a)) for u, v in corners]
            lines.append("poly " + " ".join(pt(c) for c in moved))
        return lines

    def op_circle(self, radius, forConstruction=False):
        real = self.real.circle(radius, forConstruction)
        if forConstruction:
            return W.make(real, self)
        return self.drawn(real, [f"circle {num(2 * radius)} at={pt(p)}" for p in self.points()])

    def op_ellipse(self, x_radius, y_radius, rotation_angle=0.0, sense=1, forConstruction=False, startAtCurrent=True):
        if rotation_angle or forConstruction:
            raise Unsupported("ellipse_options")
        real = self.real.ellipse(x_radius, y_radius, rotation_angle, sense, forConstruction, startAtCurrent)
        return self.drawn(real, [f"ellipse {num(2 * x_radius)} {num(2 * y_radius)} at={pt(p)}" for p in self.points()])

    def op_polygon(self, nSides, diameter, forConstruction=False, circumscribed=False):
        if forConstruction:
            raise Unsupported("polygon_construction")
        real = self.real.polygon(nSides, diameter, forConstruction, circumscribed)
        d = diameter / math.cos(math.pi / nSides) if circumscribed else diameter
        base = 180.0 / nSides if circumscribed else 0.0
        lines = [f"ngon {num(d)} {nSides} at={pt(p)} angle={num(base + p[2])}" for p in self.points()]
        return self.drawn(real, lines)

    def op_slot2D(self, length, diameter, angle=0):
        real = self.real.slot2D(length, diameter, angle)
        return self.drawn(real, [f"slot {num(length)} {num(diameter)} at={pt(p)} angle={num(angle + p[2])}" for p in self.points()])

    def segments(self, name, args, kwargs):
        before = len(self.real.ctx.pendingEdges)
        start = self.real._findFromPoint(True) if not self.pen else None
        real = getattr(self.real, name)(*args, **kwargs)
        new = real.ctx.pendingEdges[before:]
        plane = real.plane
        lines = []
        if not self.pen:
            lines.append(f"pen {pt((start.x, start.y))}")
        for e in new:
            end = plane.toLocalCoords(e.endPoint())
            kind = e.geomType()
            if kind == "LINE":
                lines.append(f"line {pt((end.x, end.y))}")
            elif kind == "CIRCLE":
                mid = plane.toLocalCoords(e.positionAt(0.5))
                lines.append(f"arc {pt((end.x, end.y))} via={pt((mid.x, mid.y))}")
            else:
                raise Unsupported("edge_" + kind.lower())
        out, change = self.emit(lines)
        change["pen"] = True
        return W.make(real, self, out, **change)

    def op_close(self):
        if not self.pen:
            raise Unsupported("close_without_path")
        real = self.real.close()
        out, change = self.emit(["close"])
        change["pen"] = False
        return W.make(real, self, out, **change)

    def op_faces(self, selector=None, tag=None):
        if selector is None:
            mine = "all"
        elif isinstance(selector, str):
            mine = face_selector_of(selector)
        else:
            raise Unsupported("faces_selector")
        real = self.real.faces(selector, tag) if tag else self.real.faces(selector)
        return W.make(real, self, sel=("faces", mine))

    def op_edges(self, selector=None, tag=None):
        if selector is not None and not isinstance(selector, str):
            raise Unsupported("edges_selector")
        real = self.real.edges(selector) if selector else self.real.edges()
        if self.sel and self.sel[0] == "faces":
            if selector is not None:
                raise Unsupported("faces_edges_selector")
            return W.make(real, self, sel=("efaces", self.sel[1]))
        mine = "all" if selector is None else edge_selector_of(selector)
        return W.make(real, self, sel=("edges", mine))

    def op_vertices(self, selector=None, tag=None):
        real = self.real.vertices(selector) if selector else self.real.vertices()
        return W.make(real, self, sel=("vertices", None))

    def op_workplane(self, *args, **kwargs):
        return W.make(self.real.workplane(*args, **kwargs), self)

    def op_placeSketch(self, *sketches):
        if len(sketches) != 1 or not isinstance(sketches[0], S):
            raise Unsupported("placeSketch")
        sk = sketches[0]
        if any(i[1] == "s" for i in sk.lines) and not any(i[1] == "a" for i in sk.lines):
            raise Unsupported("sketch_only_subtract")
        real = self.real.placeSketch(sk.real)
        return self.drawn(real, sketch_lines(sk.lines))

    def op_add(self, obj, clean=True):
        if not isinstance(obj, S):
            raise Unsupported("add")
        return self.op_placeSketch(obj)

    def op_end(self, n=1):
        raise Unsupported("end")

    def holds_solid(self):
        return bool(self.real.objects) and all(isinstance(o, real_cq.Solid) or isinstance(o, real_cq.Compound) for o in self.real.objects)

    def edge_selector(self):
        if not self.sel:
            if not self.holds_solid():
                raise Unsupported("fillet_without_selection")
            return "all"
        kind, value = self.sel
        if kind in ("efaces", "edges"):
            return value
        raise Unsupported("fillet_selection_" + kind)

    def op_shell(self, thickness, kind="arc"):
        if not self.sel or self.sel[0] != "faces" or self.sel[1] not in (">Z", "<Z", ">Z,<Z", "<Z,>Z") or thickness >= 0:
            raise Unsupported("shell_selection")
        index = max((i for i, l in enumerate(self.lines) if isinstance(l, str) and l.startswith("extrude")), default=None)
        if index is None:
            raise Unsupported("shell_without_extrude")
        lines = list(self.lines)
        lines[index] = f"s{index}: " + lines[index]
        ends = {">Z": ["end"], "<Z": ["start"], ">Z,<Z": ["end", "start"], "<Z,>Z": ["end", "start"]}[self.sel[1]]
        real = self.real.shell(thickness, kind)
        opened = ",".join(f"s{index}.{e}" for e in ends)
        return W.make(real, self, [f"shell {num(-thickness)} open={opened}"], lines=lines)

    def op_loft(self, ruled=False, combine=True, clean=True):
        mode = self.mode_of(combine) if combine else None
        run = []
        for item in reversed(self.lines):
            if isinstance(item, str) and item.split(None, 1)[0] in SKETCH_HEADS:
                run.append(item)
            else:
                break
        run.reverse()
        head = len(self.lines) - len(run)
        chunks = [[]]
        for item in run:
            if item.startswith("plane ") and chunks[-1]:
                chunks.append([])
            chunks[-1].append(item)
        if len(chunks) < 2:
            raise Unsupported("loft_sections")
        lines = list(self.lines[:head])
        for chunk in chunks:
            lines.extend(chunk)
            lines.append("section")
        real = self.real.loft(ruled, combine, clean)
        lines.append("loft" + ("" if ruled else " smooth") + (mode or ""))
        return W.make(real, self, lines=lines, plane=None)

    def op_fillet(self, radius):
        selector = self.edge_selector()
        real = self.real.fillet(radius)
        return W.make(real, self, [f"fillet {num(radius)} {selector}"])

    def op_chamfer(self, length, length2=None):
        if length2 is not None:
            raise Unsupported("chamfer_asymmetric")
        selector = self.edge_selector()
        real = self.real.chamfer(length)
        return W.make(real, self, [f"chamfer {num(length)} {selector}"])

    @staticmethod
    def mode_of(combine):
        if combine is True or combine == "a":
            return ""
        if combine in ("cut", "s"):
            return " mode=cut"
        raise Unsupported("combine_" + str(combine))

    def op_extrude(self, until, combine=True, clean=True, both=False, taper=None):
        if not isinstance(until, (int, float)):
            raise Unsupported("extrude_until")
        mode = self.mode_of(combine)
        real = self.real.extrude(until, combine, clean, both, taper)
        if combine in ("cut", "s") and self.unchanged(real):
            return self.skipped(real)
        d = num(until * 2 if both else until)
        line = f"extrude {d}" + (" both" if both else "") + (f" draft={num(taper)}" if taper else "") + mode
        out, change = self.emit([line])
        return W.make(real, self, out, **change)

    def op_cutBlind(self, until, clean=True, both=False, taper=None):
        if not isinstance(until, (int, float)):
            raise Unsupported("cutBlind_until")
        real = self.real.cutBlind(until, clean, both, taper)
        if self.unchanged(real):
            return self.skipped(real)
        d = num(until * 2 if both else until)
        line = f"extrude {d}" + (" both" if both else "") + (f" draft={num(taper)}" if taper else "") + " mode=cut"
        out, change = self.emit([line])
        return W.make(real, self, out, **change)

    def op_cutThruAll(self, clean=True, taper=0):
        if taper:
            raise Unsupported("cutThruAll_taper")
        span = self.real.largestDimension() * 2 + 1
        real = self.real.cutThruAll(clean)
        if self.unchanged(real):
            return self.skipped(real)
        out, change = self.emit([f"extrude {num(span)} both mode=cut"])
        return W.make(real, self, out, **change)

    def hole_line(self, diameter, depth, extra=""):
        pts = " ".join(pt(p) for p in self.points())
        line = f"hole {num(diameter)} {pts}" + extra
        return line + (f" depth={num(depth)}" if depth is not None else "")

    def op_hole(self, diameter, depth=None, clean=True):
        line = self.hole_line(diameter, depth)
        real = self.real.hole(diameter, depth, clean)
        if self.unchanged(real):
            return W.make(real, self)
        out, change = self.emit([line])
        return W.make(real, self, out, **change)

    def op_cboreHole(self, diameter, cboreDiameter, cboreDepth, depth=None, clean=True):
        line = self.hole_line(diameter, depth, f" cbore={num(cboreDiameter)},{num(cboreDepth)}")
        real = self.real.cboreHole(diameter, cboreDiameter, cboreDepth, depth, clean)
        if self.unchanged(real):
            return W.make(real, self)
        out, change = self.emit([line])
        return W.make(real, self, out, **change)

    def op_cskHole(self, diameter, cskDiameter, cskAngle, depth=None, clean=True):
        line = self.hole_line(diameter, depth, f" csink={num(cskDiameter)},{num(cskAngle)}")
        real = self.real.cskHole(diameter, cskDiameter, cskAngle, depth, clean)
        if self.unchanged(real):
            return W.make(real, self)
        out, change = self.emit([line])
        return W.make(real, self, out, **change)

    def op_box(self, length, width, height, centered=True, combine=True, clean=True):
        mode = self.mode_of(combine)
        flags = as_flags(centered)
        real = self.real.box(length, width, height, centered, combine, clean)
        lines = []
        for x, y, rot in self.points():
            sub = W.make(self.real, self)
            sub.real = _Stub(self.real.plane, [Location(Vector(x, y, 0))])
            lines += self.rect_for(sub, x, y, rot, length, width, flags)
            lines.append(f"extrude {num(height)}" + (" both" if flags[2] else "") + mode)
        out, change = self.emit(lines)
        return W.make(real, self, out, **change)

    def rect_for(self, sub, x, y, rot, length, width, flags):
        if abs(math.remainder(rot, 360)) > 1e-6:
            raise Unsupported("box_rotated")
        ox = 0 if flags[0] else length / 2
        oy = 0 if flags[1] else width / 2
        return [f"rect {num(length)} {num(width)} at={pt((x + ox, y + oy))}"]

    def op_cylinder(self, height, radius, direct=Vector(0, 0, 1), angle=360, centered=True, combine=True, clean=True):
        mode = self.mode_of(combine)
        if angle != 360 or tuple(round(v, 6) for v in Vector(direct).toTuple()) != (0, 0, 1):
            raise Unsupported("cylinder_options")
        flags = as_flags(centered)
        real = self.real.cylinder(height, radius, direct, angle, centered, combine, clean)
        lines = []
        for x, y, rot in self.points():
            dx = 0 if flags[0] else radius
            dy = 0 if flags[1] else radius
            lines.append(f"circle {num(2 * radius)} at={pt((x + dx, y + dy))}")
            lines.append(f"extrude {num(height)}" + (" both" if flags[2] else "") + mode)
        out, change = self.emit(lines)
        return W.make(real, self, out, **change)

    def op_revolve(self, angleDegrees=360.0, axisStart=None, axisEnd=None, combine=True, clean=True):
        mode = self.mode_of(combine)
        if axisStart is None and axisEnd is None:
            axis = "y"
        else:
            s = tuple(axisStart or (0, 0))
            e = tuple(axisEnd or (0, 1))
            d = (e[0] - s[0], e[1] - s[1])
            if abs(s[0]) < 1e-9 and abs(s[1]) < 1e-9 and abs(d[1]) < 1e-9 and abs(d[0]) > 0:
                axis = "x"
            elif abs(s[0]) < 1e-9 and abs(s[1]) < 1e-9 and abs(d[0]) < 1e-9 and d[1] > 0:
                axis = "y"
            else:
                raise Unsupported("revolve_axis")
        real = self.real.revolve(angleDegrees, axisStart, axisEnd, combine, clean)
        out, change = self.emit([f"revolve {num(angleDegrees)} axis={axis}{mode}"])
        return W.make(real, self, out, **change)

    def volume_of(self):
        try:
            return self.real.findSolid().Volume()
        except Exception:
            return None

    def unchanged(self, real):
        before, after = self.volume_of(), W.make(real, self).volume_of()
        return before is not None and after is not None and abs(before - after) <= 1e-6 * max(before, 1.0)

    def skipped(self, real):
        lines = list(self.lines)
        while lines and isinstance(lines[-1], str) and lines[-1].split(None, 1)[0] in SKETCH_HEADS:
            lines.pop()
        w = W.make(real, self, lines=lines)
        w.pkey = None
        w.pen = False
        return w

    def combine_with(self, name, other, mode, real):
        if not isinstance(other, W):
            raise Unsupported(name + "_arg")
        if not other.lines:
            raise Unsupported(name + "_empty")
        if mode == "cut" and self.unchanged(real):
            return W.make(real, self)
        if not self.lines and mode == "add":
            return W.make(real, self, list(other.lines), plane=None, pen=False)
        if not self.lines:
            raise Unsupported(name + "_into_empty")
        inline = inline_lines(other.lines, mode)
        if inline is not None:
            return W.make(real, self, inline, plane=None, pen=False)
        return W.make(real, self, [("sub", other.lines, mode)], plane=None, pen=False)

    def op_union(self, toUnion=None, clean=True, glue=False, tol=None):
        if not isinstance(toUnion, W):
            raise Unsupported("union_arg")
        real = self.real.union(toUnion.real, clean, glue, tol)
        return self.combine_with("union", toUnion, "add", real)

    def op_cut(self, toCut, clean=True, tol=None):
        if not isinstance(toCut, W):
            raise Unsupported("cut_arg")
        real = self.real.cut(toCut.real, clean, tol)
        return self.combine_with("cut", toCut, "cut", real)

    def op_intersect(self, toIntersect, clean=True, tol=None):
        if not isinstance(toIntersect, W):
            raise Unsupported("intersect_arg")
        real = self.real.intersect(toIntersect.real, clean, tol)
        return self.combine_with("intersect", toIntersect, "intersect", real)

    def op_translate(self, vec):
        real = self.real.translate(vec)
        v = Vector(vec)
        solid_on_stack = any(isinstance(o, real_cq.Shape) and not isinstance(o, (real_cq.Wire, real_cq.Edge, real_cq.Vertex, real_cq.Face)) for o in self.real.objects)
        if not solid_on_stack:
            return self.shifted_sketch(real, v)
        return W.make(real, self, [f"move {pt3(v)}"])

    def shifted_sketch(self, real, v):
        run = []
        for item in reversed(self.lines):
            if isinstance(item, str) and item.split(None, 1)[0] in SKETCH_HEADS:
                run.append(item)
            else:
                break
        run.reverse()
        if not run or any(l.startswith("plane ") for l in run[1:]):
            raise Unsupported("translate_sketch")
        if run[0].startswith("plane "):
            run = run[1:]
        if not run:
            raise Unsupported("translate_sketch")
        p = self.real.plane
        y = p.zDir.cross(p.xDir)
        origin = p.origin.add(v)
        pts = [origin, origin.add(p.xDir), origin.add(y)]
        head = list(self.lines[: len(self.lines) - len(run) - (1 if len(run) + 1 <= len(self.lines) and False else 0)])
        trailing = len([1 for _ in run])
        base = list(self.lines)
        while base and base[-1] in run and trailing:
            base.pop()
            trailing -= 1
        if base and isinstance(base[-1], str) and base[-1].startswith("plane "):
            base.pop()
        lines = base + ["plane " + " ".join(pt3(q) for q in pts)] + run
        return W.make(real, self, lines=lines, shifted=True)

    def op_rotate(self, axisStartPoint, axisEndPoint, angleDegrees):
        real = self.real.rotate(axisStartPoint, axisEndPoint, angleDegrees)
        a, b = Vector(axisStartPoint), Vector(axisEndPoint)
        name = f"ax{len(self.lines)}"
        return W.make(real, self, [f"axis {name} {pt3(a)} {pt3(b)}", f"rotate {num(angleDegrees)} axis={name}"])

    def op_mirror(self, mirrorPlane="XY", basePointVector=None, union=False, clean=True):
        if not union or not isinstance(mirrorPlane, str) or mirrorPlane not in ("XY", "YZ", "XZ"):
            raise Unsupported("mirror_options")
        base = Vector(basePointVector) if basePointVector is not None else Vector(0, 0, 0)
        axis = {"XY": base.z, "YZ": base.x, "XZ": base.y}[mirrorPlane]
        others = {"XY": (base.x, base.y), "YZ": (base.y, base.z), "XZ": (base.x, base.z)}[mirrorPlane]
        if any(abs(v) > 1e-9 for v in others):
            raise Unsupported("mirror_base")
        sign = -1 if mirrorPlane == "XZ" else 1
        real = self.real.mirror(mirrorPlane, basePointVector, union, clean)
        line = f"mirror {mirrorPlane}" + (f" offset={num(sign * axis)}" if abs(axis) > 1e-9 else "")
        return W.make(real, self, [line], plane=None)


for _name in SEGMENTS:
    def _make(name):
        def op(self, *args, **kwargs):
            return self.segments(name, args, kwargs)
        return op
    setattr(W, "op_" + _name, _make(_name))


class S:
    def __init__(self):
        self.real = real_cq.Sketch()
        self.lines = []
        self.points = [(0.0, 0.0)]

    @classmethod
    def derive(cls, real, parent, add, points):
        s = object.__new__(cls)
        s.real = real
        s.lines = parent.lines + add
        s.points = points
        return s

    def push(self, locs):
        pts = []
        for p in locs:
            if hasattr(p, "toTuple"):
                raise Unsupported("sketch_push_location")
            pts.append((float(p[0]), float(p[1])))
        return S.derive(self.real.push(locs), self, [], pts)

    def hole_or_add(self, mode):
        if mode not in ("a", "s"):
            raise Unsupported("sketch_mode")
        return mode

    def rect(self, w, h, angle=0, mode="a", tag=None):
        self.hole_or_add(mode)
        if angle:
            raise Unsupported("sketch_rect_angle")
        lines = [f"rect {num(w)} {num(h)} at={pt(p)}" for p in self.points]
        return S.derive(self.real.rect(w, h, angle, mode, tag), self, [("s", mode, lines)], [(0.0, 0.0)])

    def circle(self, r, mode="a", tag=None):
        self.hole_or_add(mode)
        lines = [f"circle {num(2 * r)} at={pt(p)}" for p in self.points]
        return S.derive(self.real.circle(r, mode, tag), self, [("s", mode, lines)], [(0.0, 0.0)])

    def polygon(self, pts, angle=0, mode="a", tag=None):
        self.hole_or_add(mode)
        if angle or len(self.points) != 1 or any(abs(v) > 1e-9 for v in self.points[0]):
            raise Unsupported("sketch_polygon_options")
        pts = [(float(p[0]), float(p[1])) for p in pts]
        if len(pts) > 1 and pts[0] == pts[-1]:
            pts = pts[:-1]
        line = "poly " + " ".join(pt(p) for p in pts)
        return S.derive(self.real.polygon(pts, angle, mode, tag), self, [("s", mode, [line])], [(0.0, 0.0)])

    def clean(self):
        return S.derive(self.real.clean(), self, [], self.points)

    def __getattr__(self, name):
        if name.startswith("__"):
            raise AttributeError(name)
        raise Unsupported("Sketch." + name)


def sketch_lines(items):
    order = [i for i in items if i[1] == "a"] + [i for i in items if i[1] == "s"]
    return [line for item in order for line in item[2]]


class _Stub:
    def __init__(self, plane, objects):
        self.plane = plane
        self.objects = objects


def inline_lines(lines, mode):
    if not lines or not all(isinstance(l, str) for l in lines):
        return None
    moves = [l for l in lines if l.startswith("move ")]
    body = [l for l in lines if not l.startswith("move ")]
    extrudes = [i for i, l in enumerate(body) if l.startswith("extrude ")]
    if len(moves) > 1 or len(extrudes) != 1 or extrudes[0] != len(body) - 1 or mode == "intersect":
        return None
    if any(l.split(None, 1)[0] not in SKETCH_HEADS - {"plane"} | {"extrude"} for l in body):
        return None
    if lines[-1].startswith("move ") is False and moves:
        return None
    if " mode=" in body[-1] or "draft=" in body[-1]:
        return None
    out = []
    if moves:
        x, y, z = [float(v) for v in moves[0].split()[1].split(",")]
        out.append(f"plane {num(x)},{num(y)},{num(z)} {num(x + 1)},{num(y)},{num(z)} {num(x)},{num(y + 1)},{num(z)}")
    else:
        out.append("plane XY")
    out.extend(body[:-1])
    out.append(body[-1] + (" mode=cut" if mode == "cut" else ""))
    return out


def flatten(lines, current, counter):
    out = []
    for item in lines:
        if isinstance(item, str):
            out.append(item)
            continue
        _, sub, mode = item
        counter[0] += 1
        name = f"b{counter[0]}"
        out.append(f"body {name}")
        out.append("plane XY")
        out.extend(flatten(sub, name, counter))
        out.append(f"combine {current} {name}" + ("" if mode == "add" else f" mode={mode}"))
    return out


def fake_module():
    mod = types.ModuleType("cadquery")
    mod.__getattr__ = lambda name: getattr(real_cq, name)
    mod.Workplane = W
    mod.Sketch = S
    return mod


class Timeout(Exception):
    pass


def on_alarm(signum, frame):
    raise Timeout()


def translate(source, seconds=40):
    fake = fake_module()
    saved = sys.modules.get("cadquery")
    sys.modules["cadquery"] = fake
    namespace = {"__name__": "program"}
    old = signal.signal(signal.SIGALRM, on_alarm)
    signal.alarm(seconds)
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            exec(compile(source, "program.py", "exec"), namespace)
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, old)
        sys.modules["cadquery"] = saved
    result = namespace.get("result")
    if not isinstance(result, W):
        candidates = [v for v in namespace.values() if isinstance(v, W)]
        if not candidates:
            raise Unsupported("no_result")
        result = candidates[-1]
    if not result.lines:
        raise Unsupported("empty_result")
    solid = result.real.val()
    box = solid.BoundingBox()
    lines = flatten(result.lines, "main", [0])
    if len(lines) > 300:
        raise Unsupported("too_long")
    return {
        "gcad": "\n".join(lines) + "\n",
        "volume": solid.Volume(),
        "bbox_min": [box.xmin, box.ymin, box.zmin],
        "bbox_max": [box.xmax, box.ymax, box.zmax],
    }


def ops_of(gcad):
    ops = []
    for line in gcad.splitlines():
        head = line.split(None, 1)[0] if line.strip() else ""
        if head.endswith(":"):
            parts = line.split(None, 2)
            head = parts[1] if len(parts) > 1 else ""
        if head:
            ops.append(head)
    return ops


def reason_of(exc):
    if isinstance(exc, Unsupported):
        return "unsupported:" + str(exc)
    if isinstance(exc, Timeout):
        return "timeout"
    return "error:" + type(exc).__name__
