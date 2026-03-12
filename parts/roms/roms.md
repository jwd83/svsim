the rules of ROM txt files:
----------------------------
roms are simple plain text files.
each line of the file is expected to have the same width
all lines end with a linebreak
each bit of a ROM is expressed as a character 1 or 0
a part with module rom connection will attempt to connect to it's text file
a module with rom_something will look for whatever comes after the underscore as it's text file in this case: something.
any unfilled regions will be assumed 0 once loaded so massive roms need not be filled wasting space.


here are example of completely blank 1, 2 and 3 bit addressable roms:


1x8.txt:

00000000
00000000


2x5.txt:

00000
00000
00000
00000

3x6.txt

000000
000000
000000
000000
000000
000000
000000
000000
