import json
import random
import sys
from collections import Counter
from pathlib import Path

from translate import reason_of, translate

DATA = Path("/home/kieran/gcad-datasets/out/zero-to-cad/obj")


def main():
    n = int(sys.argv[1])
    paths = sorted(DATA.glob("*/*/program.py"))
    random.Random(1).shuffle(paths)
    ok = 0
    reasons = Counter()
    for p in paths[:n]:
        try:
            translate(p.read_text(), 20)
            ok += 1
        except BaseException as e:
            r = reason_of(e)
            if r.startswith("error"):
                r += ":" + str(e)[:50]
            reasons[r] += 1
    print(ok, n)
    for k, v in reasons.most_common(40):
        print(v, k)


main()
