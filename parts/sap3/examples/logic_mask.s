	lda source
	and mask
	out
	lda source
	or bits
	out
	lda source
	xor toggle
	out
	hlt

.org 10
source:
	181
mask:
	15
bits:
	192
toggle:
	255
