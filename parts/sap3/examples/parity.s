	lda input
	xor shifted
	and low_bit
	out
	hlt

.org 11
input:
	105
shifted:
	82
low_bit:
	1
