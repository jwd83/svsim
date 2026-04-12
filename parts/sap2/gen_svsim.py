#!/usr/bin/env python3
"""Generate runnable SAP-2 svsim suites from the checked-in SAP-1 corpus."""

import json
import os
import shutil

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
SAP1_DIR = os.path.normpath(os.path.join(SCRIPT_DIR, "..", "sap1"))
SAP2_SOURCE = "sap2.sv"
SAP2_MICROCODE = "sap2_microcode.txt"

PROGRAMS = [
    "add3to42",
    "fib",
    "memory_traffic",
    "multiply",
    "self_modify_fetch",
    "test_jumps",
]


def load_json(path):
    with open(path) as handle:
        return json.load(handle)


def write_json(path, value):
    with open(path, "w") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def rewrite_suite(program_name):
    source_suite_path = os.path.join(SAP1_DIR, f"sap1_{program_name}.json")
    source_ram_path = os.path.join(SAP1_DIR, f"sap1_{program_name}_ram.txt")
    target_suite_path = os.path.join(SCRIPT_DIR, f"sap2_{program_name}.json")
    target_ram_name = f"sap2_{program_name}_ram.txt"
    target_ram_path = os.path.join(SCRIPT_DIR, target_ram_name)

    suite = load_json(source_suite_path)
    suite["source"] = SAP2_SOURCE
    for entry in suite.get("memory_init", []):
        if entry.get("module") == "memory" and entry.get("memory") == "data":
            entry["file"] = target_ram_name
        elif entry.get("module") == "rom" and entry.get("memory") == "data":
            entry["file"] = SAP2_MICROCODE

    shutil.copyfile(source_ram_path, target_ram_path)
    write_json(target_suite_path, suite)

    return target_ram_path, target_suite_path


def main():
    source_microcode_path = os.path.join(SAP1_DIR, "sap1_microcode.txt")
    target_microcode_path = os.path.join(SCRIPT_DIR, SAP2_MICROCODE)
    shutil.copyfile(source_microcode_path, target_microcode_path)
    print(f"wrote {target_microcode_path}")

    for program_name in PROGRAMS:
        ram_path, json_path = rewrite_suite(program_name)
        print(f"wrote {ram_path}")
        print(f"wrote {json_path}")


if __name__ == "__main__":
    main()
