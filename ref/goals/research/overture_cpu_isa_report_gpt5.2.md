# Overture CPU ISA (Turing Complete – Steam) — Reference Report

> Scope: This documents the **OVERTURE** instruction set architecture as used in the game **Turing Complete** (the early 8‑bit, 1‑byte instruction CPU you build in the campaign).

---

## 1) Architectural overview

- **Instruction width:** 8 bits (1 byte).  
- **General registers:** 6 registers, `REG 0` … `REG 5`.  
- **I/O:** one **Input** and one **Output** (often treated as if mapped like an extra register in the copy network).  
- **Canonical “fixed-function” roles (as typically used in the campaign builds):**
  - `REG 0` — immediate value target, and **jump destination** (the program counter/counter is loaded from `REG 0` on a taken condition).  
  - `REG 1` — ALU operand A.  
  - `REG 2` — ALU operand B.  
  - `REG 3` — ALU result, and the value tested by conditional jumps.  
  - `REG 4`, `REG 5` — free / long-term storage.

(These usage conventions are described in community documentation and match the campaign’s expected wiring patterns.)

---

## 2) Instruction byte layout

### 2.1 Major opcode groups (top 2 bits)

The **most-significant two bits** classify every instruction into one of four groups:

| Bits 7..6 | Group | Meaning |
|---|---|---|
| `00` | **Immediate** | Load a 6-bit literal into `REG 0` |
| `01` | **Calculate** | Perform an ALU operation on `REG 1` and `REG 2`, store in `REG 3` |
| `10` | **Copy** | Copy a value from one location to another |
| `11` | **Condition** | Conditional jump: if condition holds on `REG 3`, set counter/program counter from `REG 0` |

---

## 3) Instruction groups in detail

## 3.1 Immediate group (`00iiiiii`)

### Encoding
- **Format:** `00 i i i i i i`
- **Immediate range:** `0 .. 63` (6 bits)
- **Effect:** `REG 0 ← imm6`

### Notes
- In many practical builds, the lower 6 bits are wired directly into `REG 0` when the group select is Immediate.

---

## 3.2 Calculate group (`01xxxooo`)

### Encoding
- **Format:** `01 000 o o o`  (canonical encoding used in the published Overture ISA reference)
- **Operands:** `REG 1` and `REG 2`
- **Destination:** `REG 3`

### Semantics
`REG 3 ← ALU_OP(REG 1, REG 2)`

### Operation select (`ooo`)
The 3 least significant bits select the ALU function:

| `ooo` | Mnemonic | Decimal | Binary | Description |
|---:|---|---:|---|---|
| 0 | `or`   | 64 | `0100_0000` | bitwise OR |
| 1 | `nand` | 65 | `0100_0001` | bitwise NAND |
| 2 | `nor`  | 66 | `0100_0010` | bitwise NOR |
| 3 | `and`  | 67 | `0100_0011` | bitwise AND |
| 4 | `add`  | 68 | `0100_0100` | integer add (no carry flag in ISA) |
| 5 | `sub`  | 69 | `0100_0101` | integer subtract (two’s complement style) |

> Signedness: conditionals refer to “less than 0 / greater than 0”, implying **two’s complement signed interpretation** for those comparisons.

---

## 3.3 Copy group (`10sssddd`)

### Purpose
Copy moves a value from a **source location** to a **destination location**.

### Encoding
- **Format:** `10 s s s d d d`
- **Source field:** bits 5..3 (`sss`)  
- **Destination field:** bits 2..0 (`ddd`)

### Source codes (`sss`)
| `sss` | Source |
|---:|---|
| 0 (`000`) | `REG 0` |
| 1 (`001`) | `REG 1` |
| 2 (`010`) | `REG 2` |
| 3 (`011`) | `REG 3` |
| 4 (`100`) | `REG 4` |
| 5 (`101`) | `REG 5` |
| 6 (`110`) | `INPUT` |

*(Some implementations treat `111` as “read zero”; see “Off-label usage” below.)*

### Destination codes (`ddd`)
| `ddd` | Destination |
|---:|---|
| 0 (`000`) | `REG 0` |
| 1 (`001`) | `REG 1` |
| 2 (`010`) | `REG 2` |
| 3 (`011`) | `REG 3` |
| 4 (`100`) | `REG 4` |
| 5 (`101`) | `REG 5` |
| 6 (`110`) | `OUTPUT` |

### Example
Copy `REG 3 → REG 0`:
- group `10`
- source `REG 3` = `011`
- dest `REG 0` = `000`
- instruction: `10 011 000` = binary `1001_1000` = decimal `152`

### Off-label / non-normative convention
Many builds define an out-of-range source code (e.g., `sss=111`) to behave like **reading zero**, making it possible to “clear” a destination register by copying from the invalid source.

---

## 3.4 Condition group (`11cccccc`)

### Purpose
A Condition instruction is a **conditional jump**. The jump target is taken from `REG 0`.

### Encoding
In the canonical reference:
- the **least-significant 3 bits** select the condition
- the remaining bits are fixed such that the opcodes are contiguous.

Practically: `COND_OP = 192 + cond` where `cond` is 0..7.

### Semantics
If `COND(REG 3)` is true, then:
- `COUNTER ← REG 0` (i.e., jump to address stored in `REG 0`)

### Condition select (by opcode)
| Opcode | Mnemonic (common const name) | Meaning (test uses `REG 3`) |
|---:|---|---|
| 192 | `never` | never jump |
| 193 | `eq` | jump if `REG 3 == 0` |
| 194 | `less` | jump if `REG 3 < 0` |
| 195 | `less_eq` | jump if `REG 3 <= 0` |
| 196 | `always` | always jump |
| 197 | `not_eq` | jump if `REG 3 != 0` |
| 198 | `greater_eq` | jump if `REG 3 >= 0` |
| 199 | `greater` | jump if `REG 3 > 0` |

---

## 4) “Assembler definitions” mapping (game IDE)

The Turing Complete assembler is token → number mapping; for Overture you often define mnemonics/constants that match the above opcodes.

A typical definition set seen in the community:

- Registers:
  - `r0 000`, `r1 001`, `r2 010`, `r3 011`, `r4 100`, `r5 101`, `in 110`, `out 110`

- Instructions:
  - `imm %a(immediate|label)` → `00aaaaaa`
  - ALU ops:
    - `nand 01000000`, `or 01000001`, `and 01000010`, `nor 01000011`, `add 01000100`, `sub 01000101`
  - conditional jumps as fixed opcodes (see section 3.4)
  - copy / move can be represented as `10sssdest` patterns depending on your tokenization approach.

(Exact formatting depends on which campaign level/editor version you’re on.)

---

## 5) Quick reference tables

### 5.1 Opcode ranges by group
| Group | Binary range | Decimal range |
|---|---|---|
| Immediate | `00xxxxxx` | `0..63` |
| Calculate | `01xxxxxx` | `64..127` (only a small subset is used for canonical ALU ops) |
| Copy | `10xxxxxx` | `128..191` |
| Condition | `11xxxxxx` | `192..255` (192..199 used in canonical reference) |

### 5.2 Canonical ALU opcodes
| Mnemonic | Decimal | Binary |
|---|---:|---|
| `or` | 64 | `0100_0000` |
| `nand` | 65 | `0100_0001` |
| `nor` | 66 | `0100_0010` |
| `and` | 67 | `0100_0011` |
| `add` | 68 | `0100_0100` |
| `sub` | 69 | `0100_0101` |

---

## 6) References

- Steam community guide: “Default Instruction Set Architectures for Turing Complete” (alexanderpas)  
  https://steamcommunity.com/sharedfiles/filedetails/?id=2782647016

- Richey Ward series (helpful explanation of the same Overture encoding conventions):  
  Part 4 (CPU 1) https://richeyward.com/posts/digitron/turing-complete/04-cpu-1/  
  Part 5 (CPU 2) https://richeyward.com/posts/digitron/turing-complete/05-cpu-2/

- Steam discussion thread with Overture assembler definition snippets:  
  https://steamcommunity.com/app/1444480/discussions/0/591770590786886088/

---

*Generated on 2026-02-08.*
