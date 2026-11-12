import json
import os
import sys


def done(selection, concept_dir):
    n = 0
    for line in open(selection):
        m = json.loads(line)
        name = m["id"].replace(":", "__").replace("/", "_").replace(" ", "_")
        if os.path.exists(os.path.join(concept_dir, name + ".png")):
            n += 1
    return n


if __name__ == "__main__":
    print(done(sys.argv[1], sys.argv[2]))
