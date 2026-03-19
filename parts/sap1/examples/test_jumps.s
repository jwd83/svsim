	ldi 5
	out
subjnz:
	sub one
	out
	jnz subjnz
	jz subjnc_start
	hlt

subjnc_start:
	ldi 13
	out
subjnc:
	sub three
	out
	jc subjnc
	jnc done

done:
	hlt
.org 14
one:
	1
three:
	3
