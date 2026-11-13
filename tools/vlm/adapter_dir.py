import sys
from pathlib import Path


def adapter_dir(path):
    path = Path(path)
    if (path / "adapter_config.json").exists():
        return path
    nested = path / "policy"
    if (nested / "adapter_config.json").exists():
        return nested
    raise FileNotFoundError(f"no adapter_config.json in {path} or {nested}")


if __name__ == "__main__":
    print(adapter_dir(sys.argv[1]))
