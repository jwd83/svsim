# PySVSim Development Plan

## Vision
Build a SystemVerilog simulator for an educational digital logic game where players design hardware from NAND gates up to working CPUs.

## Current State (February 8, 2026)

### Completed
- Pure Python SystemVerilog parser and evaluator
- Combinational and sequential logic simulation
- Hierarchical module instantiation with NAND gate counting
- Bus support with bit selection and concatenation
- Truth table and waveform PNG generation
- Parallel test runner with comprehensive reporting
- Module library: gates, adders (ripple-carry and carry-select), muxes, registers, counter

### Module Library Stats
- 36 verified SystemVerilog modules in `parts/`
- 38 additional validation modules in `testing/`
- 574/574 passing JSON test cases across both regression suites
- 13,092 total NAND gates across the `parts/` library

## Immediate Goals

### Phase 1: 8-bit CPU (Next)
1. ROM module with file loading
2. Program counter with jump capability
3. Instruction decoder (4-8 instruction ISA)
4. Register file (8x8-bit)
5. Simple ALU integration
6. RAM module (256x8-bit)

See `8bitcpu-milestone.md` for detailed breakdown.

### Phase 2: ISA Implementations
- Overture ISA (from Turing Complete game)
- LEG ISA (from Turing Complete game)
- Simple stack-based architecture

### Phase 3: RV32I CPU
- Full RISC-V 32-bit integer instruction set
- Serial I/O for CLI communication
- Example programs (hello world, calculator)

See `rv32i-milestone.md` for detailed breakdown.

## Long-term Goals

### Hardware
- Port designs to Tang Nano 20K FPGA
- Verify simulation matches real hardware behavior

### Game Integration
- Build fictional game console around one of the CPUs
- Write simple games (snake, pong, etc.)
- Create puzzle-based learning progression

### Documentation
- Tutorial content for each design level
- Interactive visualizations
- Challenge problems with varying difficulty

## Design Philosophy

1. **Everything from NAND**: All modules built hierarchically from nand_gate.sv
2. **Educational focus**: Clarity over optimization
3. **Simulation first**: Get it working in Python, then port to hardware
4. **Test-driven**: Every module has comprehensive JSON tests
5. **Visual output**: PNG images for game integration
