#!/bin/bash
uv run pysvsim.py parts/testing/ > results/parts_testing.txt
uv run pysvsim.py parts/basic/ > results/parts_basic.txt
uv run pysvsim.py parts/overture/ > results/parts_overture.txt
