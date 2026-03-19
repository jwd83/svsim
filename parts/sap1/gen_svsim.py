#!/usr/bin/env python3
"""Generate svsim memory files and JSON tests for the SAP-1 CPU."""

import json
import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BUILD_DIR = os.path.join(SCRIPT_DIR, "build")

# ── Microcode generation (from make_micro_instr.py) ──────────────────────

en_write_out     = 0b1000000000000000
en_subtraction   = 0b0100000000000000
micro_done       = 0b0010000000000000
halted           = 0b0001000000000000
en_increment_pc  = 0b0000100000000000
en_write_a       = 0b0000010000000000
en_read_a        = 0b0000001000000000
en_write_b       = 0b0000000100000000
en_write_pc      = 0b0000000010000000
en_read_pc       = 0b0000000001000000
en_write_instr   = 0b0000000000100000
en_read_instr    = 0b0000000000010000
en_write_mem     = 0b0000000000001000
en_read_mem      = 0b0000000000000100
en_write_mem_adr = 0b0000000000000010
en_read_alu      = 0b0000000000000001

fetch_cycle = [
    en_read_pc  | en_write_mem_adr,
    en_read_mem | en_write_instr | en_increment_pc,
]

nop = 0


def padded(lst, desired_len, value):
    yield from lst
    yield from (value for _ in range(desired_len - len(lst)))


def generate_microcode():
    """Return list of 512 16-bit microinstruction words."""
    words = []
    for carry in [False, True]:
        for zero in [False, True]:
            instructions = [
                [micro_done],  # 0: NOP
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_a | micro_done],  # 1: LDA
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_b,
                 en_read_alu | en_write_a | micro_done],  # 2: ADD
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_b,
                 en_read_alu | en_write_a | en_subtraction | micro_done],  # 3: SUB
                [en_read_instr | en_write_mem_adr,
                 en_read_a | en_write_mem | micro_done],  # 4: STA
                [en_read_instr | en_write_a | micro_done],  # 5: LDI
                [en_read_instr | en_write_pc | micro_done],  # 6: JMP
                [((en_read_instr | en_write_pc) if carry else nop) | micro_done],  # 7: JC
                [((en_read_instr | en_write_pc) if zero else nop) | micro_done],  # 8: JZ
                [((en_read_instr | en_write_pc) if not carry else nop) | micro_done],  # 9: JNC
                [((en_read_instr | en_write_pc) if not zero else nop) | micro_done],  # a: JNZ
                [],  # b
                [],  # c
                [],  # d
                [en_read_a | en_write_out | micro_done],  # e: OUT
                [halted],  # f: HLT
            ]
            for micro_instructions in padded(instructions, 16, []):
                for micro in padded(fetch_cycle + micro_instructions, 8, nop):
                    words.append(micro)
    return words


def write_microcode_file(path):
    words = generate_microcode()
    with open(path, "w") as f:
        f.write("# SAP-1 microcode ROM (512 x 16-bit)\n")
        for i, word in enumerate(words):
            f.write(f"{i}: 0b{word:016b}\n")
    return words


# ── RAM file generation ──────────────────────────────────────────────────

def load_hex_file(hex_path):
    """Read a .hex file (one hex byte per line) into a list of ints."""
    values = []
    with open(hex_path) as f:
        for line in f:
            line = line.strip()
            if line:
                values.append(int(line, 16))
    return values


def write_ram_file(hex_path, out_path, label):
    values = load_hex_file(hex_path)
    with open(out_path, "w") as f:
        f.write(f"# SAP-1 RAM: {label}\n")
        for i, v in enumerate(values):
            f.write(f"{i}: 0x{v:02x}\n")
    return values


# ── SAP-1 simulator (for determining expected outputs) ───────────────────

def simulate_sap1(ram, microcode, max_cycles=5000):
    """Simulate the SAP-1 cycle-by-cycle, matching svsim's post-posedge model.

    svsim returns outputs AFTER the posedge and combinational settle.
    Registered outputs (out_reg_out, halted latch) reflect the new state.

    Returns a list of (cycle, event_dict) where cycle N maps to JSON step N+1
    (step 0 is the reset step).
    """
    # Registers (state after reset)
    reg_a = 0
    reg_b = 0
    reg_out = 0
    reg_pc = 0
    reg_instr = 0
    mem_adr = 0
    micro_counter = 0
    last_zero = 0
    last_carry = 0
    halted_latch = 0
    ram = list(ram)  # mutable copy

    events = []
    prev_out = 0

    for cycle in range(max_cycles):
        # Decode microinstruction (combinational, based on current register state)
        rom_addr = (last_carry << 8) | (last_zero << 7) | ((reg_instr >> 4) << 3) | micro_counter
        micro = microcode[rom_addr]

        ew_out     = (micro >> 15) & 1
        e_sub      = (micro >> 14) & 1
        m_done     = (micro >> 13) & 1
        e_halt     = (micro >> 12) & 1
        e_inc_pc   = (micro >> 11) & 1
        ew_a       = (micro >> 10) & 1
        er_a       = (micro >> 9)  & 1
        ew_b       = (micro >> 8)  & 1
        ew_pc      = (micro >> 7)  & 1
        er_pc      = (micro >> 6)  & 1
        ew_instr   = (micro >> 5)  & 1
        er_instr   = (micro >> 4)  & 1
        ew_mem     = (micro >> 3)  & 1
        er_mem     = (micro >> 2)  & 1
        ew_mem_adr = (micro >> 1)  & 1
        er_alu     = (micro >> 0)  & 1

        # Compute bus value (combinational)
        b_operand = (~reg_b & 0xff) if e_sub else reg_b
        alu_result = (reg_a + b_operand + e_sub) & 0x1ff
        alu_out = alu_result & 0xff
        carry_out = (alu_result >> 8) & 1

        mem_read = ram[mem_adr & 0xf] if (mem_adr & 0xf) < len(ram) else 0

        if er_alu:
            bus = alu_out
        elif er_instr:
            bus = reg_instr & 0x0f
        elif er_mem:
            bus = mem_read
        elif er_a:
            bus = reg_a
        elif er_pc:
            bus = reg_pc
        else:
            bus = 0

        # Posedge clk: update registers
        if ew_a:
            reg_a = bus & 0xff
        if ew_b:
            reg_b = bus & 0xff
        if ew_out:
            reg_out = bus & 0xff
        if ew_instr:
            reg_instr = bus & 0xff
        if ew_pc:
            reg_pc = bus & 0xff
        elif e_inc_pc:
            reg_pc = (reg_pc + 1) & 0xff
        if ew_mem_adr:
            mem_adr = bus & 0x0f
        if ew_mem:
            ram[mem_adr & 0xf] = bus & 0xff
        if er_alu:
            last_zero = 1 if alu_out == 0 else 0
            last_carry = carry_out
        if e_halt:
            halted_latch = 1

        # Micro counter update
        if m_done:
            micro_counter = 0
        elif micro_counter == 5:
            micro_counter = 0
        else:
            micro_counter += 1

        # Record events based on post-posedge register values
        event = {}
        if reg_out != prev_out:
            event["out_reg_out"] = reg_out
            prev_out = reg_out
        if halted_latch:
            event["halted"] = 1
            event["out_reg_out"] = reg_out
            events.append((cycle, event))
            break
        if event:
            events.append((cycle, event))

    return events


# ── JSON test generation ─────────────────────────────────────────────────

def generate_test_json(
    name, ram_file, microcode_file, ram_values, microcode,
    description=None, max_cycles=5000,
):
    """Generate a JSON test file for a SAP-1 program."""
    events = simulate_sap1(ram_values, microcode, max_cycles)

    if not events:
        print(f"  WARNING: no events found for {name}", file=sys.stderr)
        return None

    last_cycle = events[-1][0]

    # Build sequence: reset cycle, then run cycles with assertions at event points
    sequence = []

    # Cycle 0: reset
    base_inputs = {"clk": 1, "reset": 0, "en_read_external": 0, "external_value": 0}
    reset_inputs = {**base_inputs, "reset": 1}
    sequence.append({"inputs": reset_inputs, "expected": {}})

    # Build a map of cycle -> expected outputs
    event_map = {}
    for cycle, event in events:
        event_map[cycle] = event

    # Run through cycles, inserting assertions at event points
    for cycle in range(last_cycle + 1):
        step = {"inputs": base_inputs}
        if cycle in event_map:
            step["expected"] = event_map[cycle]
        else:
            step["expected"] = {}
        sequence.append(step)

    test_case = {
        "name": name,
        "sequence": sequence,
    }
    if description:
        test_case["description"] = description

    return {
        "source": "sap1.sv",
        "memory_init": [
            {"module": "memory", "memory": "data", "file": ram_file},
            {"module": "rom", "memory": "data", "file": microcode_file},
        ],
        "test_cases": [test_case],
    }


# ── Main ─────────────────────────────────────────────────────────────────

PROGRAMS = [
    {
        "name": "fib",
        "label": "Fibonacci",
        "description": "Fibonacci sequence halts with out=233",
    },
    {
        "name": "add3to42",
        "label": "Add 3 to 42",
        "description": "Repeatedly adds 3 starting from 42, outputs each step, halts at 0",
    },
    {
        "name": "multiply",
        "label": "Multiply 7x8",
        "description": "Multiplies 7*8=56 via repeated addition",
    },
]


def main():
    # Generate microcode
    microcode_path = os.path.join(SCRIPT_DIR, "sap1_microcode.txt")
    microcode = write_microcode_file(microcode_path)
    print(f"  wrote {microcode_path} ({len(microcode)} words)")

    for prog in PROGRAMS:
        hex_path = os.path.join(BUILD_DIR, f"{prog['name']}.hex")
        if not os.path.exists(hex_path):
            print(f"  SKIP {prog['name']}: {hex_path} not found", file=sys.stderr)
            continue

        ram_filename = f"sap1_{prog['name']}_ram.txt"
        ram_path = os.path.join(SCRIPT_DIR, ram_filename)
        ram_values = write_ram_file(hex_path, ram_path, prog["label"])
        print(f"  wrote {ram_path}")

        json_data = generate_test_json(
            name=prog["description"],
            ram_file=ram_filename,
            microcode_file="sap1_microcode.txt",
            ram_values=ram_values,
            microcode=microcode,
        )

        if json_data:
            json_path = os.path.join(SCRIPT_DIR, f"sap1_{prog['name']}.json")
            with open(json_path, "w") as f:
                json.dump(json_data, f, indent=2)
                f.write("\n")
            print(f"  wrote {json_path}")

            # Stats
            tc = json_data["test_cases"][0]
            n_steps = len(tc["sequence"])
            n_asserts = sum(1 for s in tc["sequence"] if s.get("expected"))
            print(f"    {n_steps} steps, {n_asserts} assertions")


if __name__ == "__main__":
    main()
