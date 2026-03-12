# Overture CPU ISA: Complete Reference

> The Overture is the first CPU architecture in *Turing Complete* (Steam), designed by developer "Stuffe" as the player's introductory processor. The name is a musical term meaning "intro." It is not based on any real-world ISA. This document consolidates two independent research reports (GPT 5.2 and Opus 4.6), with discrepancies resolved against primary sources.

---

## 1. Architecture overview

- **Instruction width:** 8 bits, fixed format.
- **Registers:** 6 general-purpose 8-bit registers (R0--R5) plus I/O at register code `110`.
- **Program counter:** 8-bit, auto-incrementing, not directly readable. Overwritten only when a Condition instruction succeeds, loading the jump target from R0.
- **Address space:** 256 instructions (2^8).
- **Word size:** 8 bits throughout (registers, memory, data path).
- **I/O:** One input port and one output port, both at register code `110` (read = input, write = output).
- **No flags register, no stack, no interrupts, no halt instruction.**

### Register encoding (hardwired roles)

The first four registers have fixed architectural roles enforced by hardware wiring, not software conventions.

| Code | Register | Role |
|------|----------|------|
| `000` | **R0** | Immediate target; jump address for Condition instructions |
| `001` | **R1** | ALU input A (first operand) |
| `010` | **R2** | ALU input B (second operand) |
| `011` | **R3** | ALU output; condition register tested by branches |
| `100` | **R4** | General purpose |
| `101` | **R5** | General purpose |
| `110` | **IN/OUT** | Input when read; Output when written |
| `111` | *(undefined)* | Reads as zero in most implementations |

> **Note:** Code `111` reading zero is a hardware side-effect (no register drives the bus at decoder output 7), not a formal ISA guarantee. See alexanderpas guide.

---

## 2. Instruction encoding

Every 8-bit value (0--255) is a valid instruction. There are no illegal opcodes.

```
Bit:  7  6  |  5  4  3  |  2  1  0
     [ OP  ] | [--- operand field ---]
```

| Bits 7--6 | Category | Bits 5--0 | Decimal range |
|-----------|----------|-----------|---------------|
| `00` | **Immediate** | 6-bit unsigned value (0--63), loaded into R0 | 0--63 |
| `01` | **Calculate** | Bits 5--3 ignored; bits 2--0 = ALU operation | 64--127 |
| `10` | **Copy** | Bits 5--3 = source register; bits 2--0 = dest register | 128--191 |
| `11` | **Condition** | Bits 5--3 ignored; bits 2--0 = condition code | 192--255 |

---

## 3. Instruction categories

### 3.1 Immediate: `00vvvvvv` -- load constant into R0

- **Effect:** `R0 <- bits[5:0]` (unsigned, range 0--63)
- The only way to introduce literal values into the machine.
- Values above 63 require ALU arithmetic (e.g., load 0 then SUB 1 yields 255).
- Assembly syntax: bare number (e.g., `5` assembles to `00000101`).

### 3.2 Calculate: `01xxx_ooo` -- ALU operation (R1 op R2 -> R3)

Bits 5--3 are ignored. Bits 2--0 select the ALU operation. Operands are always R1 and R2; the result always goes to R3.

| Bits 2--0 | Op | Decimal | Description |
|-----------|----|---------|-------------|
| `000` | **OR** | 64 | R3 <- R1 \| R2 |
| `001` | **NAND** | 65 | R3 <- ~(R1 & R2) |
| `010` | **NOR** | 66 | R3 <- ~(R1 \| R2) |
| `011` | **AND** | 67 | R3 <- R1 & R2 |
| `100` | **ADD** | 68 | R3 <- R1 + R2 (wraps at 8 bits, no carry flag) |
| `101` | **SUB** | 69 | R3 <- R1 - R2 (two's complement) |
| `110` | *(unused)* | -- | Extension slot |
| `111` | *(unused)* | -- | Extension slot |

> **Discrepancy resolved:** One source report swapped OR/NAND and NOR/AND in its assembler definitions. The correct order (OR=0, NAND=1, NOR=2, AND=3, ADD=4, SUB=5) is confirmed by the game's Logic Engine wiring, the xZise assembler source, and the alexanderpas guide.

#### Notable omissions

- **No XOR.** Synthesize via `AND(OR(a,b), NAND(a,b))` -- 3 ALU instructions plus register shuffling.
- **No NOT.** Use `NAND(x, x)` -- copy x into both R1 and R2, then NAND.
- **No shift, multiply, or divide.**
- **No carry/overflow flags.** Conditions evaluate R3 directly using signed interpretation.

### 3.3 Copy: `10_sss_ddd` -- register-to-register transfer

Bits 5--3 = source, bits 2--0 = destination, using the 3-bit register codes from section 1. This is a **copy** (source retains its value), not a move.

The instruction byte is composed by OR-ing constants:

```
const cp  128    # base opcode: 10_000_000

# Source constants (shifted left 3)
const s0  0      const s1  8      const s2  16     const s3  24
const s4  32     const s5  40     const sin 48

# Destination constants
const d0  0      const d1  1      const d2  2      const d3  3
const d4  4      const d5  5      const dout 6
```

**Examples:**
- `cp|s3|d0` = 128 + 24 + 0 = **152** (`10_011_000`) -- copy R3 to R0
- `cp|sin|dout` = 128 + 48 + 6 = **182** (`10_110_110`) -- read Input, write to Output
- Clear R1: use raw value **185** (`10_111_001`) -- source `111` reads zero (no named constant)

### 3.4 Condition: `11xxx_ccc` -- conditional jump via R3 and R0

Bits 5--3 are ignored. Bits 2--0 select a condition evaluated against R3 using **signed (two's complement) interpretation**. If true, the PC is loaded from R0.

| Bits 2--0 | Decimal | Mnemonic | Jump if R3... |
|-----------|---------|----------|---------------|
| `000` | 192 | `never` | Never (NOP equivalent) |
| `001` | 193 | `eq` | == 0 |
| `010` | 194 | `less` | < 0 (bit 7 set, values 128--255) |
| `011` | 195 | `less_eq` | <= 0 |
| `100` | 196 | `always` | Always (unconditional jump) |
| `101` | 197 | `not_eq` | != 0 |
| `110` | 198 | `greater_eq` | >= 0 |
| `111` | 199 | `greater` | > 0 |

**Bit-2 negation symmetry:** Bit 2 acts as an inversion flag. Condition `n` (0--3) is the logical complement of condition `n+4`:

| Pair | Bit 2 = 0 | Bit 2 = 1 |
|------|-----------|-----------|
| 0 / 4 | Never | Always |
| 1 / 5 | Equal | Not Equal |
| 2 / 6 | Less | Greater or Equal |
| 3 / 7 | Less or Equal | Greater |

---

## 4. Memory architecture

The Overture does not fit neatly into a Harvard or Von Neumann classification:

- **Program memory:** Single RAM block (not ROM), addressed by the 8-bit PC, holding up to 256 one-byte instructions.
- **Data memory:** None. All runtime data lives in the 6 registers.
- **No load/store instructions** in the base ISA, making it functionally Harvard: code and data occupy separate domains with no instruction to bridge them.
- The program memory is technically writable (RAM), but without load/store instructions this distinction is academic.
- Players can extend the architecture by wiring R4/R5 to external RAM address/data pins, but this goes beyond the base ISA.

---

## 5. Programming patterns

### Branch idiom (2 instructions)

Every branch requires at least 2 bytes: an Immediate to load the target address into R0, then a Condition instruction. The maximum direct jump target is address 63 (6-bit immediate limit). Higher addresses require computing the target via ALU operations.

```
10          # R0 <- 10 (immediate)
always      # PC <- R0 (unconditional jump)
```

### Conditional comparison (5 instructions)

Comparing two arbitrary values and branching:

```
cp|sX|d1    # Copy first value into R1
cp|sY|d2    # Copy second value into R2
sub         # R3 <- R1 - R2
target_addr # R0 <- jump target (immediate)
less        # Jump if R3 < 0 (i.e., X < Y)
```

### Synthesizing missing operations

| Operation | Method | Cost |
|-----------|--------|------|
| NOT x | `NAND(x, x)` -- copy x into both R1 and R2 | 3 instructions |
| XOR a,b | `AND(OR(a,b), NAND(a,b))` | 7+ instructions |
| Load value > 63 | Arithmetic from smaller values (e.g., `0` then `SUB 1` -> 255) | 4+ instructions |
| Clear register | Copy from source `111` (reads zero) | 1 instruction |

### Architectural constraints

- No stack or subroutine call/return mechanism
- No interrupts or halt instruction -- programs loop explicitly or run into uninitialized memory
- No self-modifying code (no store-to-program-memory instruction)
- The assembler supports **labels** and **constants** with `|` (OR) for composing instruction bytes

---

## 6. Complete opcode map

### 6.1 Instruction ranges

| Group | Binary | Decimal | Notes |
|-------|--------|---------|-------|
| Immediate | `00xxxxxx` | 0--63 | All 64 values are distinct literals |
| Calculate | `01xxxxxx` | 64--127 | 6 operations (64--69); 2 extension slots; bits 5--3 ignored |
| Copy | `10xxxxxx` | 128--191 | All 64 encode a distinct source/dest pair |
| Condition | `11xxxxxx` | 192--255 | 8 conditions (192--199); bits 5--3 ignored |

### 6.2 Canonical ALU opcodes

| Mnemonic | Bits 2--0 | Decimal | Binary |
|----------|-----------|---------|--------|
| OR | `000` | 64 | `0100_0000` |
| NAND | `001` | 65 | `0100_0001` |
| NOR | `010` | 66 | `0100_0010` |
| AND | `011` | 67 | `0100_0011` |
| ADD | `100` | 68 | `0100_0100` |
| SUB | `101` | 69 | `0100_0101` |

### 6.3 Assembler definitions

Complete constant definitions for the game's assembler:

```
# Registers
const r0 0
const r1 1
const r2 2
const r3 3
const r4 4
const r5 5

# Copy instruction base + source/dest offsets
const cp  128
const s0  0
const s1  8
const s2  16
const s3  24
const s4  32
const s5  40
const sin 48
const d0  0
const d1  1
const d2  2
const d3  3
const d4  4
const d5  5
const dout 6

# ALU operations
const or   64
const nand 65
const nor  66
const and  67
const add  68
const sub  69

# Condition codes
const never      192
const eq         193
const less       194
const less_eq    195
const always     196
const not_eq     197
const greater_eq 198
const greater    199
```

---

## 7. Discrepancies resolved between source reports

| Topic | GPT 5.2 | Opus 4.6 | Resolution |
|-------|---------|----------|------------|
| **ALU op order** | Section 3.2 correct; section 4 assembler defs swap OR/NAND and NOR/AND. | Correct throughout. | OR=0, NAND=1, NOR=2, AND=3, ADD=4, SUB=5. Confirmed by xZise assembler and alexanderpas guide. |
| **Register roles** | Hedges with "as typically used," implying soft conventions. | States roles are fixed and cannot be reassigned. | Roles are **hardwired**: ALU reads R1/R2, writes R3; immediate bus connects to R0; condition unit reads R3 and R0. |
| **Memory architecture** | Not discussed. | Detailed Harvard/Von Neumann analysis. | Accurate: functionally Harvard (no load/store), but program memory is RAM. |
| **XOR synthesis** | Not mentioned. | `AND(OR(a,b), NAND(a,b))`, 3 ALU ops + shuffling. | Correct: `(a|b) & ~(a&b)` is equivalent to `a XOR b`. |
| **PC details** | Not mentioned. | 8-bit PC, 256 addresses, not readable. | Correct per all sources. |
| **Condition bit-2 symmetry** | Not mentioned. | Bit 2 as negation flag with complement pairs. | Confirmed by xZise assembler enum structure. |
| **Code `111` behavior** | "Off-label usage." | "Widely used trick." | Both correct. Reads zero in most implementations; not formally specified. |
| **ISA origin** | Not discussed. | Musical term, original to game, by "Stuffe." | Confirmed by Steam community discussion. |

---

## 8. Sources

- [alexanderpas: Default ISAs for Turing Complete](https://steamcommunity.com/sharedfiles/filedetails/?id=2782647016) -- most comprehensive community reference
- [xZise/OvertureAssembler (GitHub)](https://github.com/xZise/OvertureAssembler) -- C# assembler with definitive enum definitions
- [StrategyWiki: Turing Complete/Logic Engine](https://strategywiki.org/wiki/Turing_Complete/Logic_Engine) -- confirms ALU decoder order
- [Turing Complete Wiki: Basic Assembly Programming](https://turingcomplete.wiki/wiki/Basic_Assembly_Programming) -- instruction mnemonics and tutorial
- [Richey Ward: TC Part 4 (CPU 1)](https://richeyward.com/posts/digitron/turing-complete/04-cpu-1/) -- register roles and instruction categories
- [Richey Ward: TC Part 5 (CPU 2)](https://richeyward.com/posts/digitron/turing-complete/05-cpu-2/) -- further CPU details
- [Steam Discussion: Overture/LEG history?](https://steamcommunity.com/app/1444480/discussions/0/3378284761887276303/) -- confirms name origin
- [CircuitVerse: Overture](https://circuitverse.org/users/58106/projects/overture) -- community circuit implementation

---

*Consolidated 2026-02-08 from GPT 5.2 and Opus 4.6 research reports, with discrepancies resolved via primary sources.*
