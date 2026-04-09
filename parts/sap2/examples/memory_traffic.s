	ldi 0
	sta sum
	ldi 3
	sta count
loop:
	lda sum
	add count
	sta sum
	out
	lda count
	sub one
	sta count
	jnz loop
	hlt
one:
	1
sum:
	0
count:
	0
