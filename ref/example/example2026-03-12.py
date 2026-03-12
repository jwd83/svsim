#!/usr/bin/env python3
"""Minimal embedding example for the current single-file PySVSim API."""

from pathlib import Path
import sys


REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from pysvsim import SystemVerilogParser, create_evaluator


def main() -> int:
    sv_file = REPO_ROOT / "parts" / "basic" / "and_gate.sv"

    parser = SystemVerilogParser()
    module_info = parser.parse_file(str(sv_file))
    evaluator = create_evaluator(module_info, filepath=str(sv_file))

    print(f"Loaded module: {module_info['name']}")
    print(f"Inputs: {module_info['inputs']}")
    print(f"Outputs: {module_info['outputs']}")
    print()

    test_vectors = [
        {"inA": 0, "inB": 0},
        {"inA": 0, "inB": 1},
        {"inA": 1, "inB": 0},
        {"inA": 1, "inB": 1},
    ]

    for inputs in test_vectors:
        outputs = evaluator.evaluate(inputs)
        print(f"{inputs} -> {outputs}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
