def padded(lst, desired_len, value):
	yield from lst
	yield from (value for _ in range(desired_len - len(lst)))

# SAP-3 microcode: 20 bits wide. Bits [19:16] carry the new control signals
# (output-port address select and logical ALU op selectors); bits [15:1] keep
# the SAP-2 layout; bit [0] is reserved.

en_select_output_port = 0b10000000000000000000
alu_op_xor            = 0b01000000000000000000
alu_op_or             = 0b00100000000000000000
alu_op_and            = 0b00010000000000000000
en_subtraction        = 0b00001000000000000000
micro_done            = 0b00000100000000000000
halted                = 0b00000010000000000000
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
	en_read_pc   | en_write_mem_adr,
	en_read_mem  | en_write_instr    | en_increment_pc,
]

nop = 0

for carry in [False, True]:
	for zero in [False, True]:
		instructions = [
			[ # 0: NOP
				micro_done,
			],
			[ # 1: LDA load memory to A
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_a       | micro_done,
			],
			[ # 2: ADD a = a + memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_b,
				en_read_alu   | en_write_a       | micro_done,
			],
			[ # 3: SUB a = a - memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_b,
				en_read_alu   | en_write_a | en_subtraction | micro_done,
			],
			[ # 4: STA store a to memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_a     | en_write_mem    | micro_done,
			],
			[ # 5: LDI load immediate into a
				en_read_instr | en_write_a      | micro_done,
			],
			[ # 6: JMP unconditional jump
				en_read_instr | en_write_pc     | micro_done,
			],
			[ # 7: JC jump if carry
				((en_read_instr | en_write_pc) if carry else nop) | micro_done,
			],
			[ # 8: JZ jump if zero
				((en_read_instr | en_write_pc) if zero else nop) | micro_done,
			],
			[ # 9: JNC jump if not carry
				((en_read_instr | en_write_pc) if not carry else nop) | micro_done,
			],
			[ # a: JNZ jump if not zero
				((en_read_instr | en_write_pc) if not zero else nop) | micro_done,
			],
			[ # b: AND a = a & memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_b,
				en_read_alu   | en_write_a | alu_op_and | micro_done,
			],
			[ # c: OR a = a | memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_b,
				en_read_alu   | en_write_a | alu_op_or  | micro_done,
			],
			[ # d: XOR a = a ^ memory[operand]
				en_read_instr | en_write_mem_adr,
				en_read_mem   | en_write_b,
				en_read_alu   | en_write_a | alu_op_xor | micro_done,
			],
			[ # e: OUT (memory-mapped output port at 0x10)
				en_select_output_port,
				en_read_a     | en_write_mem    | micro_done,
			],
			[ # f: HLT
				halted,
			],
		]

		for micro_instructions in padded(instructions, 16, []):
			for micro in padded(fetch_cycle + micro_instructions, 8, nop):
				print(f'{micro:020b}')
