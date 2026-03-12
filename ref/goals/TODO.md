my personal todo list

[DONE] -handle "logic" keyword in module port lists, is getting included in the port name improperly
  (also added support for 'signed', 'unsigned', and 'reg' keywords)

-make in/out labels consistent across all level files

[DONE] -make the sim throw a warning if a json file has no expect field.

-make the default input to pysvsim to be a .sv module in teh current folder with a matching .json file if no input is specified so --file is optional. append .sv if no extension is given. this way we can just do "pysvsim demux_1to2" in the terminal to run the sim on demux_1to2.sv and demux_1to2.json
