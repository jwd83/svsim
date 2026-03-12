# RV32I CPU Simulation Milestone

## Project Vision
Build a complete RV32I (RISC-V 32-bit Integer) CPU that can execute real programs and communicate with the outside world through an emulated serial port connected to the CLI.

## Current Project State

### ✅ **What We Have**
- **Comprehensive Logic Gate Library**: NAND, AND, OR, NOR, XOR, XNOR, NOT gates (1-bit and 8-bit variants)
- **Adder Family**: Half adders, full adders, ripple-carry adders (4/8/16/32/64-bit)
- **High-Performance Adders**: Carry-select adders (8/16/32/64-bit) with significant speed improvements
- **Multiplexer Library**: 2:1 muxes (1/4/8/16/32-bit) for data path selection
- **SystemVerilog Simulation Framework**: Parser, evaluator, truth table generator, comprehensive test runner
- **Testing Infrastructure**: JSON test framework, automated regression testing, performance analysis

### 📊 **Current Capabilities**
- **Gate Count**: 13,092+ NAND gates across all components
- **Simulation Speed**: Fast evaluation with 16-combination truth table limit
- **Component Verification**: 36 verified library modules (`parts/`) + 38 validation modules (`testing`) with 574/574 passing test cases
- **Hierarchical Design**: Proven scalable architecture (gates → adders → complex units)

## RV32I CPU Requirements

### 🎯 **Target Specifications**
- **Architecture**: RV32I (32-bit RISC-V Integer instruction set)
- **Instructions**: ~40 instructions (arithmetic, logic, memory, branch, jump)
- **Memory**: Unified instruction/data memory (Harvard or Von Neumann)
- **I/O**: Serial port for CLI communication (UART-style)
- **Clock**: Single-clock design with proper pipeline stages
- **Performance**: Focus on correctness over speed initially

## Milestone Roadmap

### 🏗️ **Phase 1: Memory Infrastructure (Weeks 1-2)**

#### 1.1 Basic Memory Components
- [ ] **SRAM Cell**: Basic 1-bit memory cell with read/write capability
- [ ] **Register File**: 32 × 32-bit register file for RV32I
  - Read ports: 2 (for source operands)
  - Write port: 1 (for destination)
  - Register x0 hardwired to zero
- [ ] **Program Counter (PC)**: 32-bit counter with increment/jump capability
- [ ] **Memory Unit**: Configurable SRAM for instructions and data
  - Initial size: 64KB (16K × 32-bit words)
  - Byte-addressable with proper alignment handling

#### 1.2 Memory Testing
- [ ] Register file read/write tests
- [ ] Memory addressing and data integrity tests
- [ ] PC increment and jump functionality

### 🧮 **Phase 2: Arithmetic Logic Unit (Weeks 3-4)**

#### 2.1 ALU Core Operations
- [ ] **32-bit ALU**: Combine existing adders with new functionality
  - ADD/SUB: Use 32-bit ripple-carry adder (5.6× faster simulation) with drop-in carry-select upgrade for hardware
  - AND/OR/XOR: Scale up existing 8-bit logic gates
  - SLL/SRL/SRA: Shift operations (barrel shifter or iterative)
  - SLT/SLTU: Set-less-than (signed/unsigned comparison)

#### 2.2 Comparison and Branch Logic
- [ ] **Comparator Unit**: 32-bit equality and magnitude comparison
- [ ] **Branch Decision Logic**: Zero detection, sign checking
- [ ] **Immediate Generator**: Extract and sign-extend immediates

#### 2.3 ALU Integration Testing
- [ ] Comprehensive ALU operation verification
- [ ] Timing analysis and critical path optimization
- [ ] Integration with register file

### 🔧 **Phase 3: Control Unit (Weeks 5-6)**

#### 3.1 Instruction Decoder
- [ ] **Instruction Format Parser**: Decode R/I/S/B/U/J formats
- [ ] **Control Signal Generator**: Generate 20+ control signals
  - RegWrite, MemRead, MemWrite, Branch, Jump
  - ALUOp, ALUSrc, RegDst, MemToReg, etc.
- [ ] **Opcode Decoder**: Map 7-bit opcodes to instruction types

#### 3.2 Control Logic Implementation
- [ ] **State Machine**: Simple single-cycle or multi-cycle control
- [ ] **Hazard Detection**: Basic data hazard detection (if pipelined)
- [ ] **Exception Handling**: Illegal instruction, address misalignment

### 🏛️ **Phase 4: CPU Integration (Weeks 7-8)**

#### 4.1 Datapath Integration
- [ ] **CPU Core Assembly**: Connect ALU, registers, control unit
- [ ] **Data Path Routing**: Implement all multiplexer connections
- [ ] **Timing Closure**: Ensure proper setup/hold times

#### 4.2 Instruction Implementation
- [ ] **Arithmetic**: ADD, ADDI, SUB, LUI, AUIPC
- [ ] **Logical**: AND, ANDI, OR, ORI, XOR, XORI
- [ ] **Shift**: SLL, SLLI, SRL, SRLI, SRA, SRAI
- [ ] **Compare**: SLT, SLTI, SLTU, SLTIU
- [ ] **Branch**: BEQ, BNE, BLT, BGE, BLTU, BGEU
- [ ] **Jump**: JAL, JALR
- [ ] **Memory**: LW, LH, LB, LBU, LHU, SW, SH, SB

#### 4.3 CPU Verification
- [ ] Individual instruction testing
- [ ] Simple program execution (fibonacci, factorial)
- [ ] Register file and memory integrity checks

### 📡 **Phase 5: Serial I/O Interface (Weeks 9-10)**

#### 5.1 UART Implementation
- [ ] **Serial Transmitter**: Parallel-to-serial with start/stop bits
- [ ] **Serial Receiver**: Serial-to-parallel with frame detection
- [ ] **Baud Rate Generator**: Configurable clock divider
- [ ] **Status Registers**: TX ready, RX data available, error flags

#### 5.2 Memory-Mapped I/O
- [ ] **Address Decoder**: Map serial registers to memory space
- [ ] **I/O Controller**: Handle CPU ↔ serial communication
- [ ] **Interrupt Support**: Serial interrupt capability (optional)

#### 5.3 CLI Integration
- [ ] **Python CLI Interface**: Connect simulator to terminal
- [ ] **Character I/O**: Bidirectional character transmission
- [ ] **Simple Terminal**: Echo, line editing, basic commands

### 🎮 **Phase 6: System Integration & Testing (Weeks 11-12)**

#### 6.1 Complete System Assembly
- [ ] **Full System**: CPU + Memory + Serial I/O
- [ ] **Boot Sequence**: Initialize PC, clear registers
- [ ] **System Testing**: End-to-end program execution

#### 6.2 Test Programs
- [ ] **Hello World**: Print "Hello, World!" via serial
- [ ] **Echo Program**: Read input, echo back to terminal
- [ ] **Calculator**: Simple arithmetic calculator via CLI
- [ ] **Memory Test**: Read/write memory patterns

#### 6.3 Performance Analysis
- [ ] **Instruction Throughput**: Instructions per second
- [ ] **Memory Bandwidth**: Data transfer rates
- [ ] **Serial Performance**: Baud rate and latency analysis

## Technical Challenges & Solutions

### 🚧 **Expected Challenges**

1. **Memory Timing**: Ensuring proper setup/hold for memory operations
   - **Solution**: Add pipeline registers, implement wait states if needed

2. **Control Complexity**: Managing 20+ control signals correctly
   - **Solution**: Use systematic control unit design, extensive testing

3. **Serial Timing**: Baud rate generation and synchronization
   - **Solution**: Use proven UART designs, implement clock domain crossing

4. **Debug Visibility**: Understanding what's happening inside CPU
   - **Solution**: Extensive logging, waveform dumps, single-step capability

5. **Simulation Performance**: Large designs may simulate slowly
   - **Solution**: Use ripple-carry adders instead of carry-select (5.6× faster), optimize gate count over speed

### 💡 **Optimization Strategies**

1. **Leverage Existing Assets**: Reuse ripple-carry adders, muxes, logic gates for optimal simulation performance
2. **Incremental Development**: Build and test each component individually
3. **Comprehensive Testing**: Use existing test framework for new components
4. **Modular Design**: Keep CPU components cleanly separated
5. **Drop-In Hardware Optimization**: Identical interfaces allow swapping ripple-carry → carry-select for hardware implementation
6. **Simulation Performance**: Use ripple-carry adders (5.6× faster than carry-select in software simulation)

### 🔄 **Simulation vs Hardware Strategy**

**Simulation Phase** (Development & Testing):
- Use `adder_32bit` (ripple-carry) for 5.6× faster iteration
- 480 NAND gates vs 2,332 for carry-select
- Faster compilation, testing, and debugging

**Hardware Phase** (FPGA/ASIC Implementation):
- Drop-in replace with `adder_cs_32bit` (carry-select)
- Identical interface: `module adder_XX_32bit (inA, inB, inCarry, outSum, outCarry)`
- Significant speed improvement in real hardware
- No CPU architecture changes required

## Success Metrics

### 🎯 **Milestone Completion Criteria**

- **✅ Phase 1 Complete**: Register file passes all read/write tests
- **✅ Phase 2 Complete**: ALU executes all RV32I arithmetic/logic operations
- **✅ Phase 3 Complete**: Control unit generates correct signals for all instructions
- **✅ Phase 4 Complete**: CPU executes simple programs (loops, branches, memory access)
- **✅ Phase 5 Complete**: Serial I/O sends/receives characters to/from CLI
- **✅ Phase 6 Complete**: "Hello World" program runs and prints via serial

### 📈 **Performance Targets**

- **Simulation Speed**: >1000 instructions/second in Python simulator
- **Gate Count**: <100K NAND gate equivalents for full system
- **Memory**: Support for 64KB+ of program/data memory
- **Serial Rate**: 9600+ baud communication with CLI
- **Reliability**: 99.9%+ instruction execution accuracy

## Resource Estimates

### 🛠️ **Component Complexity**
- **New SystemVerilog Modules**: ~25-30 additional modules
- **Lines of Code**: ~5000-8000 lines of new SystemVerilog
- **Test Cases**: ~500+ new test cases across all components
- **Development Time**: ~12 weeks with focused effort

### 📚 **Dependencies**
- **RISC-V ISA Spec**: Reference for instruction encoding/behavior
- **UART Standards**: RS-232 protocol for serial communication
- **Python Libraries**: Serial I/O libraries for CLI interface
- **Test Programs**: Simple C programs compiled to RV32I assembly

This roadmap provides a systematic path from our current sophisticated arithmetic/logic foundation to a complete, working RV32I CPU with real-world I/O capability. Each phase builds incrementally on previous work while maintaining our high standards for testing and verification.
