import sys
import argparse

VALUE = 0
LABEL = 1

PROG_SIZE = 16

parser = argparse.ArgumentParser(description='Assemble a program')
parser.add_argument('infile', type=open, nargs='?', default=sys.stdin,
					help='Assembly input file, default stdin')
parser.add_argument('-o', dest='outfile', type=str, default=sys.stdout,
					help='The output hex file, default stdout')
args = parser.parse_args()

def decode_value(text):
	if text.isdigit():
		return (int(text), VALUE)
	else:
		return (text, LABEL)

class Instruction:
	def __init__(self, args, encoding):
		self.args = args
		self.encoding = encoding

	def encode(self, args):
		if len(args) != self.args:
			raise Exception('Wrong number of args, expected %d, got %d' % \
				(self.args, len(args)))

		value, typ = (0, VALUE) if self.args == 0 else decode_value(args[0])

		if typ == LABEL:
			return lambda labels: (self.encoding << 4) + labels[value]
		else:
			return (self.encoding << 4) + value

instructions = {
	'nop': Instruction(0,  0),
	'lda': Instruction(1,  1),
	'add': Instruction(1,  2),
	'sub': Instruction(1,  3),
	'sta': Instruction(1,  4),
	'ldi': Instruction(1,  5),
	'jmp': Instruction(1,  6),
	'jc':  Instruction(1,  7),
	'jz':  Instruction(1,  8),
	'jnc': Instruction(1,  9),
	'jnz': Instruction(1, 10),
	'and': Instruction(1, 11),
	'or':  Instruction(1, 12),
	'xor': Instruction(1, 13),
	'out': Instruction(0, 14),
	'hlt': Instruction(0, 15),
}

def parse_instruction(text):
	values = text.split()
	mnemonic = values[0]
	if mnemonic.isdecimal():
		return int(mnemonic)
	instruction = instructions[mnemonic.lower()]
	try:
		return instruction.encode(values[1:])
	except:
		raise Exception("Failed to parse instruction '%s'" % (text))

encoders = []
labels = {}
for line in args.infile:
	line = line.rstrip()
	if line == '':
		continue

	if line.startswith(' ') or line.startswith('\t'):
		encoders.append(parse_instruction(line.strip()))
	elif line.endswith(':'):
		labels[line[:-1]] = len(encoders)
	elif line.startswith('.org'):
		encoders.extend(0 for _ in range(int(line.split()[1]) - len(encoders)))
	else:
		raise Exception('unable to decode line "%s"' % line)

if len(encoders) > PROG_SIZE:
	print("Program too big. Uses %d instructions. Max size %d" % (len(encoders), PROG_SIZE), file=sys.stderr)
	sys.exit(1)
encoders.extend([0] * (PROG_SIZE - len(encoders)))

out = open(args.outfile, 'w') if type(args.outfile) is str else args.outfile

for encoder in encoders:
	encoded = encoder if (type(encoder) == int) else encoder(labels)
	print(f'{encoded:02x}', file=out)
