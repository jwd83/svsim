#!/usr/bin/env python3
"""Scaffold generator metadata for the future SAP-2 corpus."""

import argparse
import json
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
EXAMPLES_DIR = os.path.join(SCRIPT_DIR, "examples")
SOURCE_FILE = os.path.join(SCRIPT_DIR, "sap2.sv")
MICROCODE_FILE = os.path.join(SCRIPT_DIR, "sap2_microcode.txt")


def discover_examples():
    return sorted(
        os.path.splitext(name)[0]
        for name in os.listdir(EXAMPLES_DIR)
        if name.endswith(".s")
    )


def planned_artifacts(example_name):
    return {
        "example": os.path.join("examples", f"{example_name}.s"),
        "ram_file": f"sap2_{example_name}_ram.txt",
        "json_file": f"sap2_{example_name}.json",
    }


def build_manifest():
    return {
        "source": os.path.basename(SOURCE_FILE),
        "microcode": os.path.basename(MICROCODE_FILE),
        "examples": [planned_artifacts(name) for name in discover_examples()],
        "status": "scaffold",
        "notes": [
            "SAP-2 generation is intentionally deferred until the shared-bus core exists.",
            "This script reserves artifact names and keeps the copied example corpus discoverable.",
        ],
    }


def main():
    parser = argparse.ArgumentParser(
        description="Report the planned SAP-2 artifact layout."
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit the scaffold manifest as JSON",
    )
    args = parser.parse_args()

    manifest = build_manifest()
    if args.json:
        print(json.dumps(manifest, indent=2))
        return

    print("SAP-2 scaffold manifest")
    print(f"source: {manifest['source']}")
    print(f"microcode: {manifest['microcode']}")
    for note in manifest["notes"]:
        print(f"note: {note}")
    for example in manifest["examples"]:
        print(
            f"example: {example['example']} -> "
            f"{example['ram_file']}, {example['json_file']}"
        )


if __name__ == "__main__":
    main()
