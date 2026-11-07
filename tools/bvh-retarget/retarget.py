import argparse
import json
import math
import re
import sys
from pathlib import Path

import numpy as np

BVH_TO_GCAD = np.array([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]])


def rot_axis(axis, deg):
    a = np.asarray(axis, float)
    a = a / np.linalg.norm(a)
    t = math.radians(deg)
    k = np.array([[0, -a[2], a[1]], [a[2], 0, -a[0]], [-a[1], a[0], 0]])
    return np.eye(3) + math.sin(t) * k + (1 - math.cos(t)) * k @ k


def between(u, v):
    u = u / np.linalg.norm(u)
    v = v / np.linalg.norm(v)
    c = float(np.dot(u, v))
    axis = np.cross(u, v)
    s = np.linalg.norm(axis)
    if s < 1e-9:
        if c > 0:
            return np.eye(3)
        perp = np.cross(u, [1, 0, 0])
        if np.linalg.norm(perp) < 1e-6:
            perp = np.cross(u, [0, 1, 0])
        return rot_axis(perp, 180)
    return rot_axis(axis, math.degrees(math.atan2(s, c)))


def axis_angle(r):
    c = max(-1.0, min(1.0, (np.trace(r) - 1) / 2))
    ang = math.acos(c)
    if ang < 1e-6:
        return 0.0, (1.0, 0.0, 0.0)
    if math.pi - ang < 1e-4:
        w, v = np.linalg.eigh((r + r.T) / 2)
        axis = v[:, np.argmax(w)]
    else:
        axis = np.array([r[2, 1] - r[1, 2], r[0, 2] - r[2, 0], r[1, 0] - r[0, 1]])
    axis = axis / np.linalg.norm(axis)
    return math.degrees(ang), tuple(axis)


def scaled(r, k):
    ang, axis = axis_angle(r)
    return rot_axis(axis, ang * k) if ang else np.eye(3)


class Bvh:
    def __init__(self, path):
        text = Path(path).read_text().split()
        self.names, self.parent, self.offset, self.channels = [], [], [], []
        stack, i = [], 0
        while text[i] != "MOTION":
            tok = text[i]
            if tok in ("ROOT", "JOINT"):
                self.names.append(text[i + 1])
                self.parent.append(stack[-1] if stack else None)
                i += 2
            elif tok == "End":
                self.names.append(f"_end{len(self.names)}")
                self.parent.append(stack[-1])
                self.channels.append([])
                i += 2
            elif tok == "{":
                stack.append(len(self.names) - 1)
                i += 1
            elif tok == "}":
                stack.pop()
                i += 1
            elif tok == "OFFSET":
                self.offset.append(np.array([float(x) for x in text[i + 1:i + 4]]))
                i += 4
            elif tok == "CHANNELS":
                n = int(text[i + 1])
                self.channels.append(text[i + 2:i + 2 + n])
                i += 2 + n
            else:
                i += 1
        frames = int(text[i + 2])
        self.dt = float(text[i + 5])
        self.frames = np.array([float(x) for x in text[i + 6:]]).reshape(frames, -1)
        self.index = {n: k for k, n in enumerate(self.names)}
        self.rest = []
        for j in range(len(self.names)):
            p = self.parent[j]
            self.rest.append(self.offset[j] + (self.rest[p] if p is not None else 0))

    def pose(self, f):
        row = self.frames[f]
        col = 0
        rots, pos = [], []
        for j in range(len(self.names)):
            local = np.eye(3)
            off = self.offset[j].copy()
            for ch in self.channels[j]:
                v = row[col]
                col += 1
                if ch.endswith("position"):
                    off["XYZ".index(ch[0])] = v
                else:
                    local = local @ rot_axis(np.eye(3)["XYZ".index(ch[0])], v)
            p = self.parent[j]
            if p is None:
                rots.append(local)
                pos.append(off)
            else:
                rots.append(rots[p] @ local)
                pos.append(pos[p] + rots[p] @ off)
        g = BVH_TO_GCAD
        return [g @ r @ g.T for r in rots], [g @ p for p in pos]

    def rest_pos(self, name):
        return BVH_TO_GCAD @ self.rest[self.index[name]]

    def tip(self, a, b):
        return self.rest_pos(b) - self.rest_pos(a)


def read_rig(path):
    bones = {}
    order = []
    for line in Path(path).read_text().splitlines():
        m = re.match(r"bone (\w+)(.*)", line)
        if m:
            args = dict(re.findall(r"(\w+)=(\S+)", m.group(2)))
            name = m.group(1)
            parent = args.get("parent")
            head = (
                np.array([float(x) for x in args["head"].split(",")])
                if "head" in args
                else bones[parent]["tail"]
            )
            tail = np.array([float(x) for x in args["tail"].split(",")])
            bones[name] = {"parent": parent, "head": head, "tail": tail}
            order.append(name)
        m = re.match(r"mirror x of=(\w+)", line)
        if m and m.group(1) in bones:
            group = [m.group(1)]
            for n in order:
                if bones[n]["parent"] in group:
                    group.append(n)
            flip = np.array([-1.0, 1.0, 1.0])
            for n in group:
                b = bones[n]
                bones[n.replace("_l", "_r")] = {
                    "parent": b["parent"].replace("_l", "_r") if b["parent"] in group else b["parent"],
                    "head": b["head"] * flip,
                    "tail": b["tail"] * flip,
                }
            order.extend(n.replace("_l", "_r") for n in group)
    return bones, order


def fk(bones, order, local):
    world_r, world_t = {}, {}
    for n in order:
        b = bones[n]
        p = b["parent"]
        if p is None:
            world_r[n] = local[n]
            world_t[n] = b["head"]
        else:
            world_r[n] = world_r[p] @ local[n]
            world_t[n] = world_t[p] + world_r[p] @ (b["head"] - bones[p]["head"])
    return world_r, world_t


def lowest(bones, wr, wt, contacts):
    out = math.inf
    for bone, end, pad in contacts:
        b = bones[bone]
        at = b["tail"] if end == "tail" else b["head"] if end == "head" else (b["head"] + b["tail"]) / 2
        out = min(out, (wt[bone] + wr[bone] @ (at - b["head"]))[2] - pad)
    return out


def clear_arm(r_arm, rest_dir, chest_r, side, least):
    d = chest_r.T @ (r_arm @ rest_dir)
    want = math.sin(math.radians(least))
    if d[0] * side >= want:
        return r_arm
    flat = np.array([0.0, d[1], d[2]])
    if np.linalg.norm(flat) < 1e-6:
        flat = np.array([0.0, 0.0, -1.0])
    fixed = flat / np.linalg.norm(flat) * math.sqrt(1 - want * want)
    fixed[0] = side * want
    return chest_r @ between(d, fixed) @ chest_r.T @ r_arm


def best_loop(poses, keys, lo, hi, span):
    best = None
    n = len(poses)
    for i in range(int(n * lo), int(n * hi)):
        for j in range(i + span[0], min(n, i + span[1])):
            d = sum(axis_angle(poses[i][b].T @ poses[j][b])[0] ** 2 for b in keys)
            if best is None or d < best[0]:
                best = (d, i, j)
    if best is None:
        raise SystemExit("loop search found no span: widen search or span")
    return best[1], best[2]


def number(v, places):
    text = f"{v:.{places}f}".rstrip("0").rstrip(".")
    return "0" if text in ("", "-0") else text


def retarget(name, clip, config, base, bones, order):
    src = Bvh(base / clip["file"])
    source_of = config["map"]
    aim = config.get("aim", {})
    upright = config.get("upright", {})
    root = config["root"]
    scale = config["scale"]
    facing = config["facing"]
    contacts = config.get("contacts", [[root, "head", 0.0]])
    fps = config.get("fps", 30)
    floor = min(src.rest_pos(n)[2] for n in scale["floor"])
    k = bones[scale["bone"]]["head"][2] / (src.rest_pos(scale["source"])[2] - floor)
    yaw = None
    fitted = {}
    frames = []
    for f in range(len(src.frames)):
        rots, pos = src.pose(f)
        if yaw is None:
            across = pos[src.index[facing[0]]] - pos[src.index[facing[1]]]
            yaw = rot_axis([0, 0, 1], -math.degrees(math.atan2(across[1], across[0])))
            for b in order:
                a, tip = source_of[b]
                fitted[b] = between(bones[b]["tail"] - bones[b]["head"], yaw @ src.tip(a, tip))
        world = {}
        for b in order:
            world[b] = yaw @ rots[src.index[source_of[b][0]]] @ yaw.T @ fitted[b]
        for b, (a, tip) in aim.items():
            want = yaw @ (pos[src.index[tip]] - pos[src.index[a]])
            world[b] = between(world[b] @ (bones[b]["tail"] - bones[b]["head"]), want) @ world[b]
        for b, (keep, limit) in upright.items() if clip.get("upright", True) else ():
            d = world[b] @ (bones[b]["tail"] - bones[b]["head"])
            pitch = math.degrees(math.atan2(-d[1], d[2]))
            target = max(-limit, min(limit, pitch * keep))
            world[b] = rot_axis([1, 0, 0], target - pitch) @ world[b]
        for b, s in clip.get("amp", {}).items():
            p = bones[b]["parent"]
            parent = world[p] if p else np.eye(3)
            world[b] = parent @ scaled(parent.T @ world[b], s)
        for arm, side in config.get("arms", {}).items():
            rest_dir = bones[arm]["tail"] - bones[arm]["head"]
            rest_dir = rest_dir / np.linalg.norm(rest_dir)
            chest = world[bones[arm]["parent"]]
            world[arm] = clear_arm(world[arm], rest_dir, chest, side, clip.get("arm_clear", 20))
        local = {}
        for b in order:
            p = bones[b]["parent"]
            local[b] = world[b] if p is None else world[p].T @ world[b]
        frames.append((local, max(0.0, min(p[2] for p in pos) - floor) * k))
    if clip.get("trim"):
        lo, hi = clip["trim"]
        frames = frames[lo:hi]
    if clip.get("loop"):
        i, j = best_loop(
            [f[0] for f in frames],
            config.get("loop_bones", order),
            *clip.get("search", (0.1, 0.7)),
            clip.get("span", (12, 35)),
        )
        frames = frames[i:j] + [frames[i]]
        print(name, "loop", i, j, file=sys.stderr)
    lift_mode = clip.get("lift", "ground")
    out = [f"clip {name} fps={fps}"]
    for t, (local, target) in enumerate(frames):
        wr, wt = fk(bones, order, local)
        low = lowest(bones, wr, wt, contacts)
        lift = {"ground": target - low, "plant": -low}.get(lift_mode, 0.0)
        time = number(t / fps, 4)
        for b in order:
            ang, axis = axis_angle(local[b])
            ax = ",".join(f"{v:.4f}" for v in axis)
            out.append(f"key t={time} bone={b} rotate={ang:.3f} axis={ax}")
        out.append(f"key t={time} bone={root} move=0,0,{lift:.3f}")
    print(name, "frames", len(frames), file=sys.stderr)
    return out


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Retarget BVH takes onto a gcad rig as clip and key lines"
    )
    parser.add_argument("config", help="JSON: rig, map, root, scale, facing, clips and the rest")
    parser.add_argument("output", help="the .gcad file of clip and key lines to write")
    args = parser.parse_args(argv)
    path = Path(args.config).resolve()
    config = json.loads(path.read_text())
    base = path.parent
    bones, order = read_rig(base / config["rig"])
    missing = [b for b in order if b not in config["map"]]
    if missing:
        raise SystemExit(f"map has no source joints for {', '.join(missing)}")
    lines = []
    for name in sorted(config["clips"]):
        lines += retarget(name, config["clips"][name], config, base, bones, order)
    Path(args.output).write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
