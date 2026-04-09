	ldi 0
	sta patched
	jmp patched
	hlt
	hlt
	hlt
patched:
	hlt
	ldi 7
	out
	hlt
