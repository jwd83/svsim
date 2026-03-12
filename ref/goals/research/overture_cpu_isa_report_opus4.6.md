# The Overture CPU: a complete ISA reference

**The Overture is an 8-bit RISC processor with a fixed 1-byte instruction format, 6 registers, and just four instruction categories—Immediate, Calculate, Copy, and Condition.** It is the first CPU architecture players build in *Turing Complete*, the logic-gate-to-computer puzzle game on Steam. Invented by developer "Stuffe" (not based on any real-world ISA, despite surface similarities to the Intel 8008), the name "Overture" is a musical term meaning "intro" or "beginning," reflecting its role as the player's introductory CPU. The architecture is deliberately minimal: all ALU operations are hardwired to specific registers, immediates are limited to 6 bits, and there are no flags, no stack, and no data memory in the base design.

## Eight bits encode everything the CPU can do

Every Overture instruction is exactly **8 bits (1 byte)** wide. The **two most significant bits (bits 7–6)** determine which of four instruction categories to execute. The remaining 6 bits carry the operand, and their interpretation varies by category:

```
Bit:  7  6 │ 5  4  3 │ 2  1  0
     [OP  ] │ [--- operand field ---]
```

| Bits 7–6 | Category | Remaining 6 bits encode | Decimal range |
|----------|----------|------------------------|---------------|
| `00` | **Immediate** | 6-bit unsigned value (0–63) | 0–63 |
| `01` | **Calculate** | Bits 5–3 unused; bits 2–0 = ALU op | 64–127 |
| `10` | **Copy** | Bits 5–3 = source reg; bits 2–0 = dest reg | 128–191 |
| `11` | **Condition** | Bits 5–3 unused; bits 2–0 = condition code | 192–255 |

This encoding means every possible 8-bit value (0–255) maps to a valid instruction. There is no illegal opcode—even the "unused" bit fields in Calculate and Condition are simply ignored (don't-care bits).

## Six registers with rigid roles plus I/O

The Overture has **six 8-bit registers** (R0–R5), plus a virtual seventh register slot for I/O. Each is identified by a 3-bit code. Critically, **R0–R3 have fixed architectural roles** that cannot be reassigned, while R4 and R5 are truly general-purpose.

| 3-bit code | Register | Designated role |
|-----------|----------|----------------|
| `000` | **R0** | Receives all immediate values; holds **jump target address** for branch instructions |
| `001` | **R1** | **ALU input 1** — first operand for all Calculate instructions |
| `010` | **R2** | **ALU input 2** — second operand for all Calculate instructions |
| `011` | **R3** | **ALU output** — receives all Calculate results; also the **condition register** checked by branch instructions |
| `100` | **R4** | General purpose (no designated use) |
| `101` | **R5** | General purpose (no designated use) |
| `110` | **IN / OUT** | **Input** when read as source; **Output** when written as destination |
| `111` | *(unassigned)* | Reading returns zero in most implementations (useful as a "clear register" trick) |

The **program counter** auto-increments each cycle and is **not directly accessible** as a register. It can only be overridden by a successful Condition (jump) instruction, which loads the PC from R0. The address space is **8 bits wide**, supporting up to **256 instructions**.

## Four instruction categories define the entire ISA

### Immediate: `00vvvvvv` — load a constant into R0

The 6 low-order bits encode an unsigned value from **0 to 63**, which is loaded directly into R0. This is the only way to introduce literal values. To load values above 63, arithmetic is required—for example, loading 0 and then subtracting 1 yields 255 (−1 in two's complement). Assembly syntax is typically just the bare number (e.g., `5` assembles to `00000101`) or a mnemonic like `li 5`.

### Calculate: `01xxx_ooo` — ALU operations on R1 and R2, result to R3

Bits 5–3 are don't-care. The **3 least significant bits** select among six ALU operations. The operands are always R1 and R2; the result always goes to R3. There is **no register flexibility**—to add two arbitrary values, you must first Copy them into R1 and R2.

| Bits 2–0 | Decimal | Operation | Description |
|----------|---------|-----------|-------------|
| `000` | 64 | **OR** | R3 ← R1 \| R2 (bitwise) |
| `001` | 65 | **NAND** | R3 ← ~(R1 & R2) |
| `010` | 66 | **NOR** | R3 ← ~(R1 \| R2) |
| `011` | 67 | **AND** | R3 ← R1 & R2 |
| `100` | 68 | **ADD** | R3 ← R1 + R2 (wraps at 8 bits, no carry) |
| `101` | 69 | **SUB** | R3 ← R1 − R2 (two's complement) |
| `110` | — | *(unused)* | Available for player extensions |
| `111` | — | *(unused)* | Available for player extensions |

Several notable omissions define the programming challenge. **There is no XOR**: it must be synthesized as `AND(OR(a,b), NAND(a,b))`, costing three instructions plus register shuffling. **There is no NOT**: achievable via `NAND(x, x)` by copying a value into both R1 and R2. **There are no shift, multiply, or divide operations.** ADD and SUB produce no carry or overflow flags—the architecture has **no flags register whatsoever**. Conditions are evaluated directly from R3's signed value.

### Copy: `10_sss_ddd` — register-to-register transfer

Bits 5–3 encode the **source** and bits 2–0 encode the **destination**, using the same 3-bit register codes from the table above. This is a copy, not a move—the source retains its value. The instruction byte is assembled by OR-ing the base opcode with shifted source and destination constants:

```
const cp  128   # base: 10000000
const s0  0     const s1  8     const s2  16    const s3  24
const s4  32    const s5  40    const in  48
const d0  0     const d1  1     const d2  2     const d3  3
const d4  4     const d5  5     const out 6
```

For example, `cp|s3|d0` = 128 + 24 + 0 = **152** (`10011000`), which copies R3 → R0. Writing `cp|in|out` reads from Input and writes to Output in a single instruction. An undocumented but widely used trick: source `111` reads zero in most implementations, so `10111_ddd` effectively clears the destination register.

### Condition: `11xxx_ccc` — conditional jump using R3 and R0

Bits 5–3 are don't-care. The 3 low-order bits select a **condition evaluated against R3** (signed interpretation). If the condition is true, the program counter is set to the value in **R0** (the jump target). If false, execution continues sequentially.

| Bits 2–0 | Decimal | Mnemonic | Jump if R3... |
|----------|---------|----------|---------------|
| `000` | 192 | `never` | Never (NOP equivalent) |
| `001` | 193 | `eq` | == 0 |
| `010` | 194 | `less` | < 0 (signed negative) |
| `011` | 195 | `less_eq` | ≤ 0 |
| `100` | 196 | `always` | Always (unconditional jump) |
| `101` | 197 | `not_eq` | ≠ 0 |
| `110` | 198 | `greater_eq` | ≥ 0 |
| `111` | 199 | `greater` | > 0 |

The encoding has an elegant symmetry: **bit 2 acts as a negation flag**. Each condition in positions 0–3 is the logical complement of the corresponding condition in positions 4–7 (Never↔Always, Equal↔Not Equal, Less↔Greater or Equal, Less or Equal↔Greater). The standard jump idiom requires two instructions: first load the target address into R0 with an Immediate, then execute the Condition. For example, to jump unconditionally to address 10:

```
10        # R0 ← 10 (immediate)
always    # PC ← R0
```

This means every branch costs **2 bytes** of program memory and the maximum direct jump target is address **63** (the immediate limit). Reaching higher addresses requires computing the target via ALU operations.

## Memory architecture sits between Harvard and Von Neumann

The Overture's memory classification is debated in the community, and for good reason—it doesn't fit neatly into either category. **Program memory** is a single RAM block addressed by the 8-bit program counter, holding up to **256 one-byte instructions**. In the base architecture, **there is no separate data RAM**. All runtime data lives exclusively in the six registers.

The program memory is technically a RAM component (not ROM), meaning it *could* be written to, leading some community members to classify it as **Von Neumann**. However, the base ISA provides **no load/store instructions** for accessing data memory, making it functionally **Harvard-like** in practice: program memory holds code, and registers hold data, with no instruction capable of bridging the two at runtime.

Players can extend the architecture by wiring R4 and R5 to an external RAM block's address and data pins, creating pseudo-memory-mapped data storage—but this goes beyond the base ISA specification. The **word size is uniformly 8 bits** across all registers, memory, and the data path. I/O consists of a single input port and a single output port, both mapped to register encoding `110`, distinguished only by read (source) vs. write (destination) context.

## Programming patterns reveal the architecture's constraints

The Overture's rigid register assignments create distinctive programming patterns. Since ALU operations are locked to R1/R2→R3 and jumps read from R0/R3, even simple tasks require careful register choreography. Consider comparing two values and branching: you must Copy both values into R1 and R2, execute SUB to place the difference in R3, load the branch target into R0 via Immediate (which overwrites whatever was in R0), and then execute the Condition instruction—a minimum of **5 instructions** for a conditional branch on arbitrary data.

The architecture has **no stack, no subroutine call/return mechanism, no interrupts, and no halt instruction**. Programs either loop explicitly or run off the end of populated memory into whatever values happen to be in uninitialized addresses. Self-modifying code is theoretically possible if the player wires the architecture to allow writes to program memory, but the base ISA does not support it.

The game's assembler supports **labels** (`label name`) for jump targets and **constants** (`const name value`) for instruction mnemonics, with the `|` (bitwise OR) operator used to compose instruction bytes from their component fields. Some community assemblers (like xZise's OvertureAssembler on GitHub) add convenience features such as automatically inserting the required Immediate instruction before a labeled jump.

## Conclusion

The Overture is a masterclass in minimal CPU design—**just 4 instruction types, 6 ALU operations, and 8 condition codes** encode the entire ISA into a single byte. Its most distinctive design choices are the hardwired register roles (R0 for immediates/jump targets, R1/R2 for ALU inputs, R3 for ALU output/condition checks) and the complete absence of flags, stacks, or memory access instructions. These constraints force players to deeply understand register allocation, instruction sequencing, and how to synthesize missing operations from primitives—which is precisely the pedagogical point. The two unused ALU slots (`110`, `111`) and the don't-care bits in Calculate and Condition instructions leave deliberate room for player extensions, foreshadowing the more capable LEG architecture that follows later in the game.