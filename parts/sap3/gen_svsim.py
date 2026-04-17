#!/usr/bin/env python3
"""Generate svsim memory files and JSON tests for the SAP-3 CPU.

SAP-3 extends SAP-2 with:
  - logical ALU ops (AND / OR / XOR)
  - memory-mapped output port at address 0x10 (replaces the dedicated out_r)
  - 20-bit microcode word

The harness-visible top contract (clk / reset / en_read_external /
external_value -> out_reg_out / halted) is unchanged, so the JSON suite
shape matches the parts/sap1 and parts/sap2 pattern.
"""

import json
import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
EXAMPLES_DIR = os.path.join(SCRIPT_DIR, "examples")

# ── Microcode generation ─────────────────────────────────────────────────

en_select_output_port = 0b10000000000000000000
alu_op_xor            = 0b01000000000000000000
alu_op_or             = 0b00100000000000000000
alu_op_and            = 0b00010000000000000000
en_subtraction        = 0b00001000000000000000
micro_done            = 0b00000100000000000000
halted_flag           = 0b00000010000000000000
en_increment_pc       = 0b00000001000000000000
en_write_a            = 0b00000000100000000000
en_read_a             = 0b00000000010000000000
en_write_b            = 0b00000000001000000000
en_write_pc           = 0b00000000000100000000
en_read_pc            = 0b00000000000010000000
en_write_instr        = 0b00000000000001000000
en_read_instr         = 0b00000000000000100000
en_write_mem          = 0b00000000000000010000
en_read_mem           = 0b00000000000000001000
en_write_mem_adr      = 0b00000000000000000100
en_read_alu           = 0b00000000000000000010

fetch_cycle = [
    en_read_pc  | en_write_mem_adr,
    en_read_mem | en_write_instr | en_increment_pc,
]

nop = 0


def padded(lst, desired_len, value):
    yield from lst
    yield from (value for _ in range(desired_len - len(lst)))


def generate_microcode():
    """Return list of 512 20-bit microinstruction words."""
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
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_b,
                 en_read_alu | en_write_a | alu_op_and | micro_done],  # b: AND
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_b,
                 en_read_alu | en_write_a | alu_op_or | micro_done],  # c: OR
                [en_read_instr | en_write_mem_adr,
                 en_read_mem | en_write_b,
                 en_read_alu | en_write_a | alu_op_xor | micro_done],  # d: XOR
                [en_select_output_port,
                 en_read_a | en_write_mem | micro_done],  # e: OUT
                [halted_flag],  # f: HLT
            ]
            for micro_instructions in padded(instructions, 16, []):
                for micro in padded(fetch_cycle + micro_instructions, 8, nop):
                    words.append(micro)
    return words


def write_microcode_file(path):
    words = generate_microcode()
    with open(path, "w") as f:
        f.write("# SAP-3 microcode ROM (512 x 20-bit)\n")
        for i, word in enumerate(words):
            f.write(f"{i}: 0b{word:020b}\n")
    return words


# ── Assembler ────────────────────────────────────────────────────────────

INSTRUCTIONS = {
    "nop": (0, 0), "lda": (1, 1), "add": (1, 2), "sub": (1, 3),
    "sta": (1, 4), "ldi": (1, 5), "jmp": (1, 6), "jc":  (1, 7),
    "jz":  (1, 8), "jnc": (1, 9), "jnz": (1, 10),
    "and": (1, 11), "or": (1, 12), "xor": (1, 13),
    "out": (0, 14), "hlt": (0, 15),
}

PROG_SIZE = 16


def assemble(source_path):
    """Assemble a SAP-3 source file and return a 16-byte RAM image."""
    encoders = []
    labels = {}

    def parse_line(text):
        parts = text.split()
        if parts[0].isdecimal():
            return int(parts[0])
        argc, encoding = INSTRUCTIONS[parts[0].lower()]
        rest = parts[1:]
        if len(rest) != argc:
            raise Exception(f"Wrong arg count for {parts[0]!r}")
        if argc == 0:
            return (encoding << 4)
        if rest[0].isdigit():
            return (encoding << 4) + int(rest[0])
        return ("LABEL", encoding, rest[0])

    with open(source_path) as f:
        for raw in f:
            line = raw.rstrip()
            if line == "":
                continue
            if line.startswith(" ") or line.startswith("\t"):
                encoders.append(parse_line(line.strip()))
            elif line.endswith(":"):
                labels[line[:-1]] = len(encoders)
            elif line.startswith(".org"):
                target = int(line.split()[1])
                encoders.extend(0 for _ in range(target - len(encoders)))
            else:
                raise Exception(f"unable to decode line {line!r}")

    if len(encoders) > PROG_SIZE:
        raise Exception(f"program too big: {len(encoders)} > {PROG_SIZE}")
    encoders.extend([0] * (PROG_SIZE - len(encoders)))

    resolved = []
    for enc in encoders:
        if isinstance(enc, tuple) and enc[0] == "LABEL":
            _, encoding, label = enc
            resolved.append((encoding << 4) + labels[label])
        else:
            resolved.append(enc)
    return resolved


def write_ram_file(ram_values, out_path, label):
    with open(out_path, "w") as f:
        f.write(f"# SAP-3 RAM: {label}\n")
        for i, v in enumerate(ram_values):
            f.write(f"{i}: 0x{v:02x}\n")


# ── SAP-3 simulator (for determining expected outputs) ───────────────────

def simulate(ram, microcode, max_cycles=5000):
    """Simulate the SAP-3 cycle-by-cycle, matching svsim's post-posedge model."""
    reg_a = 0
    reg_b = 0
    reg_pc = 0
    reg_instr = 0
    mem_adr = 0       # 5-bit in the real design
    out_port = 0
    micro_counter = 0
    last_zero = 0
    last_carry = 0
    halted_latch = 0
    ram = list(ram)

    events = []
    prev_out = 0

    for cycle in range(max_cycles):
        rom_addr = (last_carry << 8) | (last_zero << 7) | ((reg_instr >> 4) << 3) | micro_counter
        micro = microcode[rom_addr]

        e_select_out = (micro >> 19) & 1
        e_op_xor     = (micro >> 18) & 1
        e_op_or      = (micro >> 17) & 1
        e_op_and     = (micro >> 16) & 1
        e_sub        = (micro >> 15) & 1
        m_done       = (micro >> 14) & 1
        e_halt       = (micro >> 13) & 1
        e_inc_pc     = (micro >> 12) & 1
        ew_a         = (micro >> 11) & 1
        er_a         = (micro >> 10) & 1
        ew_b         = (micro >>  9) & 1
        ew_pc        = (micro >>  8) & 1
        er_pc        = (micro >>  7) & 1
        ew_instr     = (micro >>  6) & 1
        er_instr     = (micro >>  5) & 1
        ew_mem       = (micro >>  4) & 1
        er_mem       = (micro >>  3) & 1
        ew_mem_adr   = (micro >>  2) & 1
        er_alu       = (micro >>  1) & 1

        # ALU combinational output
        b_operand = (~reg_b & 0xff) if e_sub else reg_b
        sum_result = (reg_a + b_operand + e_sub) & 0x1ff
        if e_op_and:
            alu_out = reg_a & reg_b
            carry_out = 0
        elif e_op_or:
            alu_out = reg_a | reg_b
            carry_out = 0
        elif e_op_xor:
            alu_out = reg_a ^ reg_b
            carry_out = 0
        else:
            alu_out = sum_result & 0xff
            carry_out = (sum_result >> 8) & 1

        # Memory combinational read
        if mem_adr & 0x10:
            mem_read = out_port
        else:
            mem_read = ram[mem_adr & 0xf]

        # Bus (priority follows the z-resolution: a single driver wins)
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

        # Posedge clk: update registers (address_register update precedes
        # mem write within the memory module, but both read the OLD value
        # since non-blocking; we mirror that with `new_mem_adr`).
        new_mem_adr = mem_adr
        if e_select_out:
            new_mem_adr = 0x10
        elif ew_mem_adr:
            new_mem_adr = bus & 0x0f
        if ew_mem:
            if mem_adr & 0x10:
                out_port = bus & 0xff
            else:
                ram[mem_adr & 0xf] = bus & 0xff
        mem_adr = new_mem_adr

        if ew_a:
            reg_a = bus & 0xff
        if ew_b:
            reg_b = bus & 0xff
        if ew_instr:
            reg_instr = bus & 0xff
        if ew_pc:
            reg_pc = bus & 0xff
        elif e_inc_pc:
            reg_pc = (reg_pc + 1) & 0xff
        if er_alu:
            last_zero = 1 if alu_out == 0 else 0
            last_carry = carry_out
        if e_halt:
            halted_latch = 1

        if m_done:
            micro_counter = 0
        elif micro_counter == 5:
            micro_counter = 0
        else:
            micro_counter += 1

        event = {}
        if out_port != prev_out:
            event["out_reg_out"] = out_port
            prev_out = out_port
        if halted_latch:
            event["halted"] = 1
            event["out_reg_out"] = out_port
            events.append((cycle, event))
            break
        if event:
            events.append((cycle, event))

    return events


# ── JSON test generation ─────────────────────────────────────────────────

def generate_test_json(name, description, ram_file, microcode_file,
                       ram_values, microcode, max_cycles=5000):
    events = simulate(ram_values, microcode, max_cycles)
    if not events:
        print(f"  WARNING: no events for {name}", file=sys.stderr)
        return None

    last_cycle = events[-1][0]
    base_inputs = {"clk": 1, "reset": 0, "en_read_external": 0, "external_value": 0}
    reset_inputs = {**base_inputs, "reset": 1}

    sequence = [{"inputs": reset_inputs, "expected": {}}]
    event_map = {cyc: ev for cyc, ev in events}
    for cycle in range(last_cycle + 1):
        step = {"inputs": base_inputs, "expected": event_map.get(cycle, {})}
        sequence.append(step)

    return {
        "source": "sap3.sv",
        "memory_init": [
            {"module": "memory", "memory": "data", "file": ram_file},
            {"module": "rom", "memory": "data", "file": microcode_file},
        ],
        "test_cases": [{
            "name": name,
            "description": description,
            "sequence": sequence,
        }],
    }


# ── Main ─────────────────────────────────────────────────────────────────

PROGRAMS = [
    {
        "name": "add3to42",
        "source": "add3to42.s",
        "label": "Add 3 to 42",
        "description": "Repeatedly adds 3 starting from 42, writes to memory-mapped output port, halts at 0",
    },
    {
        "name": "fib",
        "source": "fib.s",
        "label": "Fibonacci",
        "description": "Fibonacci sequence via memory-mapped output port halts when carry sets",
    },
    {
        "name": "logic_mask",
        "source": "logic_mask.s",
        "label": "Logic AND/OR/XOR",
        "description": "Exercises AND, OR, and XOR against a base value and emits each result",
    },
    {
        "name": "parity",
        "source": "parity.s",
        "label": "XOR parity",
        "description": "Computes the low-bit parity of 0x69 ^ 0x52 via XOR then AND",
    },
]


def main():
    microcode_path = os.path.join(SCRIPT_DIR, "sap3_microcode.txt")
    microcode = write_microcode_file(microcode_path)
    print(f"  wrote {microcode_path} ({len(microcode)} words)")

    for prog in PROGRAMS:
        src_path = os.path.join(EXAMPLES_DIR, prog["source"])
        if not os.path.exists(src_path):
            print(f"  SKIP {prog['name']}: {src_path} not found", file=sys.stderr)
            continue

        ram_values = assemble(src_path)
        ram_filename = f"sap3_{prog['name']}_ram.txt"
        ram_path = os.path.join(SCRIPT_DIR, ram_filename)
        write_ram_file(ram_values, ram_path, prog["label"])
        print(f"  wrote {ram_path}")

        json_data = generate_test_json(
            name=prog["description"],
            description=prog["label"],
            ram_file=ram_filename,
            microcode_file="sap3_microcode.txt",
            ram_values=ram_values,
            microcode=microcode,
        )
        if json_data:
            json_path = os.path.join(SCRIPT_DIR, f"sap3_{prog['name']}.json")
            with open(json_path, "w") as f:
                json.dump(json_data, f, indent=2)
                f.write("\n")
            tc = json_data["test_cases"][0]
            n_steps = len(tc["sequence"])
            n_asserts = sum(1 for s in tc["sequence"] if s.get("expected"))
            print(f"  wrote {json_path} ({n_steps} steps, {n_asserts} assertions)")


if __name__ == "__main__":
    main()
