#!/usr/bin/env python3
"""
SystemVerilog Simulator for Game Development

A pure Python SystemVerilog simulator that parses basic combinational logic
modules and generates truth tables. Supports testing with JSON test cases.

Usage:
    python pysvsim.py --file <verilog_file> [--test <json_file>] [--max-combinations N]
    python pysvsim.py <file_or_folder> [--sequential] [--workers N]
"""

import argparse
import contextlib
import io
import json
import multiprocessing
import os
import re
import sys
import time
import traceback
from concurrent.futures import ProcessPoolExecutor, as_completed
from itertools import product
from pathlib import Path
from typing import Dict, List, Tuple, Any, Optional
from PIL import Image, ImageDraw, ImageFont, ImageFilter


# Global module cache to prevent repeated parsing of the same modules
GLOBAL_MODULE_CACHE = {}


def parse_sv_range(range_expr: str) -> Tuple[int, int, int]:
    """Parse a SystemVerilog range expression like [7:0]."""
    range_match = re.match(r"\[\s*(\d+)\s*:\s*(\d+)\s*\]$", range_expr.strip())
    if not range_match:
        raise ValueError(f"Invalid range expression: {range_expr}")
    msb = int(range_match.group(1))
    lsb = int(range_match.group(2))
    width = abs(msb - lsb) + 1
    return msb, lsb, width


def _parse_memory_value(value_str: str) -> int:
    """Parse a memory value string supporting binary, hex, and decimal formats."""
    value_clean = value_str.replace("_", "")
    # Plain binary (only 0s and 1s, no prefix)
    if re.fullmatch(r"[01]+", value_clean):
        return int(value_clean, 2)
    # Use Python's auto-detection for 0b, 0x, 0o prefixes and decimal
    return int(value_clean, 0)


def load_memory_txt_file(file_path: str, word_width: int, depth: int) -> List[int]:
    """Load plain-text memory initialization data."""
    memory = [0] * depth
    if not file_path:
        return memory

    with open(file_path, "r", encoding="utf-8") as mem_file:
        lines = mem_file.readlines()

    current_address = 0
    max_word = (1 << word_width) - 1 if word_width > 0 else 0

    for raw_line in lines:
        line = raw_line.strip()
        if not line or line.startswith("#") or line.startswith("//"):
            continue
        # Strip inline comments
        for marker in ("//", "#"):
            pos = line.find(marker)
            if pos >= 0:
                line = line[:pos].strip()
        if not line:
            continue

        # Optional address override syntax: <addr>:<value>
        if ":" in line:
            addr_str, value_str = line.split(":", 1)
            address = int(addr_str.strip(), 0)
            value_str = value_str.strip()
        else:
            value_str = line
            address = current_address

        if address < 0 or address >= depth:
            continue

        memory[address] = _parse_memory_value(value_str) & max_word
        current_address = address + 1

    return memory


def normalize_memory_bindings(test_data: Any, test_dir: str, default_module: str = "") -> List[Dict[str, Any]]:
    """Normalize memory init entries from test JSON."""
    if not isinstance(test_data, dict):
        return []

    normalized: List[Dict[str, Any]] = []

    def _add_binding(entry: Dict[str, Any], mem_type: str):
        if not isinstance(entry, dict):
            return
        file_value = entry.get("file") or entry.get("path")
        if not file_value:
            return
        file_path = file_value
        if not os.path.isabs(file_path):
            file_path = os.path.normpath(os.path.join(test_dir, file_path))
        normalized.append(
            {
                "type": (entry.get("type") or mem_type or "ram").lower(),
                "module": entry.get("module") or default_module or "",
                "instance": entry.get("instance") or entry.get("instance_path") or "",
                "memory": entry.get("memory") or entry.get("name") or "",
                "file": file_path,
            }
        )

    memory_inits = test_data.get("memory_init", [])
    if isinstance(memory_inits, list):
        for init_entry in memory_inits:
            _add_binding(init_entry, init_entry.get("type", "ram") if isinstance(init_entry, dict) else "ram")

    def _process_entries(entries: Any, mem_type: str):
        """Process dict or list of memory entries."""
        if isinstance(entries, dict):
            _add_binding(entries, mem_type)
        elif isinstance(entries, list):
            for entry in entries:
                _add_binding(entry, mem_type)

    memory_files = test_data.get("memory_files", {})
    if isinstance(memory_files, dict):
        for mem_type in ("rom", "ram"):
            _process_entries(memory_files.get(mem_type, []), mem_type)

    # Backward/short-form support: top-level rom/ram blocks.
    for mem_type in ("rom", "ram"):
        _process_entries(test_data.get(mem_type), mem_type)

    # Program harness convention:
    # pgm_<name> automatically binds overture_fetch.rom to <name>.txt
    # when no explicit memory bindings were provided in JSON.
    if not normalized and default_module.startswith("pgm_"):
        program_name = default_module[4:]
        if program_name:
            program_file = os.path.normpath(os.path.join(test_dir, f"{program_name}.txt"))
            if os.path.exists(program_file):
                normalized.append(
                    {
                        "type": "rom",
                        "module": "overture_fetch",
                        "instance": "",
                        "memory": "rom",
                        "file": program_file,
                    }
                )

    return normalized


def clear_module_cache():
    """Clear the global module cache. Useful for testing or when modules change."""
    global GLOBAL_MODULE_CACHE
    GLOBAL_MODULE_CACHE.clear()


class SystemVerilogParser:
    """Parser for a subset of SystemVerilog focused on basic combinational logic."""

    def __init__(self):
        self._reset_parse_state()

    def _reset_parse_state(self):
        self.module_name = ""
        self.inputs = []
        self.outputs = []
        self.wires = []
        self.bus_info = {}  # Track bus widths and ranges
        self.assignments = {}
        self.slice_assignments = (
            []
        )  # Track bus slice assignments like out[31:24] = in[7:0]
        self.concat_assignments = (
            []
        )  # Track concatenation assignments like {w, x, y, z} = expr
        self.instantiations = []
        self.sequential_blocks = []  # Track always_ff blocks and other sequential logic
        self.combinational_blocks = []  # Track always_comb blocks
        self.clock_signals = set()  # Track identified clock signals
        self.memory_arrays = {}
        self.filepath = ""

    def parse_file(self, filepath: str) -> Dict[str, Any]:
        """
        Parse a SystemVerilog file and extract module information.

        Args:
            filepath: Path to the .sv file

        Returns:
            Dictionary containing module info: name, inputs, outputs, assignments
        """
        self._reset_parse_state()
        self.filepath = os.path.abspath(filepath)
        try:
            with open(self.filepath, "r", encoding="utf-8") as f:
                content = f.read()
        except FileNotFoundError:
            raise FileNotFoundError(f"SystemVerilog file not found: {filepath}")
        except Exception as e:
            raise Exception(f"Error reading file {filepath}: {e}")

        return self._parse_content(content)

    def _parse_content(self, content: str) -> Dict[str, Any]:
        """Parse the SystemVerilog content and extract module components."""
        # Remove comments and clean up
        content = self._remove_comments(content)
        content = " ".join(content.split())  # Normalize whitespace

        # Extract module declaration
        module_match = re.search(r"module\s+(\w+)\s*\((.*?)\)\s*;", content, re.DOTALL)
        if not module_match:
            raise ValueError("No valid module declaration found")

        self.module_name = module_match.group(1)
        port_list = module_match.group(2)

        # Parse ports
        self._parse_ports(content, port_list)

        # Parse wire declarations
        self._parse_wires(content)

        # Parse reg/logic signal declarations
        self._parse_signal_declarations(content)

        # Parse memory array declarations
        self._parse_memory_arrays(content)

        # Parse assign statements
        self._parse_assignments(content)

        # Parse module instantiations
        self._parse_instantiations(content)
        
        # Parse sequential logic blocks
        self._parse_sequential_blocks(content)

        # Parse combinational logic blocks
        self._parse_combinational_blocks(content)

        return {
            "name": self.module_name,
            "inputs": self.inputs.copy(),
            "outputs": self.outputs.copy(),
            "assignments": self.assignments.copy(),
            "slice_assignments": self.slice_assignments.copy(),
            "concat_assignments": self.concat_assignments.copy(),
            "instantiations": self.instantiations.copy(),
            "sequential_blocks": self.sequential_blocks.copy(),
            "combinational_blocks": self.combinational_blocks.copy(),
            "clock_signals": list(self.clock_signals),
            "bus_info": self.bus_info.copy(),
            "memory_arrays": self.memory_arrays.copy(),
            "filepath": self.filepath,
        }

    def _remove_comments(self, content: str) -> str:
        """Remove single-line (//) and multi-line (/* */) comments."""
        # Remove single-line comments
        content = re.sub(r"//.*$", "", content, flags=re.MULTILINE)
        # Remove multi-line comments
        content = re.sub(r"/\*.*?\*/", "", content, flags=re.DOTALL)
        return content

    def _parse_ports(self, content: str, port_list: str):
        """Parse input and output port declarations, including buses."""
        # First, try to parse from the port list in the module declaration
        # This handles cases where ports are declared in the module header
        self._parse_port_list(port_list)

        # Port list parsing is complete. Additional port declarations in module body
        # would be for internal signals, not module ports, so we skip content-based
        # port parsing to avoid conflicts.

    def _parse_port_list(self, port_list: str):
        """Parse the port list from the module declaration header."""
        # Remove extra whitespace and handle multi-line declarations
        port_list = " ".join(port_list.split())

        # Parse by finding all input/output sections
        # Split the port list into sections starting with input or output
        sections = []
        current_section = ""
        current_type = None

        # Tokenize the port list, including modifiers that can appear after input/output
        tokens = re.findall(
            r"\b(?:input|output|wire|logic|reg|signed|unsigned)\b|\[[^\]]+\]|\w+|,", port_list
        )

        i = 0
        while i < len(tokens):
            token = tokens[i]

            if token in ["input", "output"]:
                # Save previous section if exists
                if current_type and current_section:
                    self._parse_port_section(current_type, current_section)

                # Start new section
                current_type = token
                current_section = ""
            elif token in ["wire", "logic", "reg", "signed", "unsigned"]:
                # Skip modifier keywords - they're optional and don't affect simulation functionality
                pass
            elif token.startswith("[") and token.endswith("]"):
                # Bus specification
                current_section += " " + token
            elif token == ",":
                # Comma separator
                current_section += token
            elif token.isalnum() or "_" in token:
                # Signal name
                current_section += " " + token

            i += 1

        # Process the last section
        if current_type and current_section:
            self._parse_port_section(current_type, current_section)

    def _parse_port_section(self, port_type: str, section: str):
        """Parse a single port section (input or output)."""
        section = section.strip()

        # Check if this is a bus declaration
        bus_match = re.match(r"\s*\[(\d+):(\d+)\]\s*(.+)", section)
        if bus_match:
            # Bus declaration
            msb, lsb, port_names = bus_match.groups()
            msb, lsb = int(msb), int(lsb)
            width = abs(msb - lsb) + 1

            # Parse signal names
            names = [name.strip() for name in port_names.split(",") if name.strip()]

            for port_name in names:
                self.bus_info[port_name] = {"msb": msb, "lsb": lsb, "width": width}

                if port_type == "input":
                    self.inputs.append(port_name)
                elif port_type == "output":
                    self.outputs.append(port_name)
        else:
            # Single-bit declarations
            names = [name.strip() for name in section.split(",") if name.strip()]

            for port_name in names:
                if port_name not in self.bus_info:
                    self.bus_info[port_name] = {"msb": 0, "lsb": 0, "width": 1}

                    if port_type == "input":
                        self.inputs.append(port_name)
                    elif port_type == "output":
                        self.outputs.append(port_name)

    def _parse_wires(self, content: str):
        """Parse wire declarations, including bus wires and initialized wires."""
        # Handle bus wire declarations with initialization like: wire [24:0] v1 = expression;
        bus_wire_init_pattern = r"wire\s+\[(\d+):(\d+)\]\s+(\w+)\s*=\s*([^;]+)\s*;"
        bus_wire_init_declarations = re.findall(bus_wire_init_pattern, content)

        for msb, lsb, wire_name, expression in bus_wire_init_declarations:
            msb, lsb = int(msb), int(lsb)
            width = abs(msb - lsb) + 1
            self.bus_info[wire_name] = {"msb": msb, "lsb": lsb, "width": width}
            self.wires.append(wire_name)
            # Treat the initialization as an assignment
            self.assignments[wire_name] = expression.strip()

        # Handle single-bit wire declarations with initialization like: wire temp = expression;
        single_wire_init_pattern = r"wire\s+(\w+)\s*=\s*([^;]+)\s*;"
        single_wire_init_declarations = re.findall(single_wire_init_pattern, content)

        for wire_name, expression in single_wire_init_declarations:
            if wire_name not in self.bus_info:
                self.bus_info[wire_name] = {"msb": 0, "lsb": 0, "width": 1}
                self.wires.append(wire_name)
                # Treat the initialization as an assignment
                self.assignments[wire_name] = expression.strip()

        # Handle bus wire declarations like: wire [3:0] temp;
        bus_wire_pattern = r"wire\s+\[(\d+):(\d+)\]\s+(\w+)\s*;"
        bus_wire_declarations = re.findall(bus_wire_pattern, content)

        for msb, lsb, wire_name in bus_wire_declarations:
            msb, lsb = int(msb), int(lsb)
            width = abs(msb - lsb) + 1
            self.bus_info[wire_name] = {"msb": msb, "lsb": lsb, "width": width}
            self.wires.append(wire_name)

        # Handle single-bit wire declarations like: wire temp1, temp2;
        single_wire_pattern = r"wire\s+(?!\[)([\w,\s]+)\s*;"
        single_wire_declarations = re.findall(single_wire_pattern, content)

        for wire_list in single_wire_declarations:
            # Split by comma and clean up whitespace
            wires = [wire.strip() for wire in wire_list.split(",") if wire.strip()]
            for wire_name in wires:
                if wire_name not in self.bus_info:
                    self.bus_info[wire_name] = {"msb": 0, "lsb": 0, "width": 1}
                    self.wires.append(wire_name)

    def _is_valid_signal_name(self, name: str) -> bool:
        """Check if a name is a valid signal identifier."""
        reserved_keywords = {"input", "output", "wire", "logic", "reg", "signed", "unsigned"}
        if not name or "[" in name:
            return False
        if not re.fullmatch(r"\w+", name):
            return False
        return name not in reserved_keywords

    def _parse_signal_declarations(self, content: str):
        """Parse reg/logic declarations (excluding memory arrays)."""
        bus_decl_pattern = (
            r"\b(?:reg|logic)\b(?:\s+(?:signed|unsigned))?\s+"
            r"\[(\d+):(\d+)\]\s+([^;]+)\s*;"
        )
        for msb_str, lsb_str, name_list in re.findall(bus_decl_pattern, content):
            msb = int(msb_str)
            lsb = int(lsb_str)
            width = abs(msb - lsb) + 1
            for raw_name in name_list.split(","):
                name = raw_name.strip()
                if self._is_valid_signal_name(name) and name not in self.bus_info:
                    self.bus_info[name] = {"msb": msb, "lsb": lsb, "width": width}

        single_decl_pattern = (
            r"\b(?:reg|logic)\b(?:\s+(?:signed|unsigned))?\s+"
            r"(?!\[)([^;]+)\s*;"
        )
        for name_list in re.findall(single_decl_pattern, content):
            for raw_name in name_list.split(","):
                name = raw_name.strip()
                if self._is_valid_signal_name(name) and name not in self.bus_info:
                    self.bus_info[name] = {"msb": 0, "lsb": 0, "width": 1}

    def _parse_memory_arrays(self, content: str):
        """Parse memory array declarations like reg [7:0] mem [255:0];."""
        memory_pattern = (
            r"\b(?:reg|logic)\b(?:\s+(?:signed|unsigned))?\s*"
            r"(\[[^\]]+\])?\s+(\w+)\s*(\[[^\]]+\])\s*;"
        )
        declarations = re.findall(memory_pattern, content)

        for packed_range, memory_name, unpacked_range in declarations:
            try:
                if packed_range:
                    word_msb, word_lsb, word_width = parse_sv_range(packed_range)
                else:
                    word_msb, word_lsb, word_width = 0, 0, 1

                index_msb, index_lsb, depth = parse_sv_range(unpacked_range)
                self.memory_arrays[memory_name] = {
                    "word_msb": word_msb,
                    "word_lsb": word_lsb,
                    "word_width": word_width,
                    "index_msb": index_msb,
                    "index_lsb": index_lsb,
                    "depth": depth,
                }
            except Exception:
                continue

    def _parse_assignments(self, content: str):
        """Parse assign statements and build assignment expressions."""
        # Enhanced pattern to capture all assignment types including concatenation targets
        assign_pattern = r"assign\s+([^=]+)\s*=\s*([^;]+)\s*;"
        assignments = re.findall(assign_pattern, content)

        for output_signal, expression in assignments:
            # Clean up both output_signal and expression
            output_signal = output_signal.strip()
            expression = expression.strip()

            # Check if this is a concatenation assignment like {w, x, y, z} = expr
            if output_signal.startswith("{") and output_signal.endswith("}"):
                # Parse concatenation target
                targets = output_signal[1:-1].strip()  # Remove braces
                target_list = [t.strip() for t in targets.split(",")]
                self.concat_assignments.append(
                    {"targets": target_list, "expression": expression}
                )
            # Check if this is a bus slice assignment like out[31:24] = in[7:0]
            else:
                slice_match = re.match(r"(\w+)\[(\d+):(\d+)\]", output_signal)
                if slice_match:
                    # This is a bus slice assignment
                    signal_name = slice_match.group(1)
                    msb = int(slice_match.group(2))
                    lsb = int(slice_match.group(3))
                    self.slice_assignments.append(
                        {
                            "signal": signal_name,
                            "msb": msb,
                            "lsb": lsb,
                            "expression": expression,
                        }
                    )
                else:
                    # Regular assignment
                    base_signal_match = re.match(
                        r"(\w+)(?:\[\d+:\d+\])?", output_signal
                    )
                    if base_signal_match:
                        base_signal = base_signal_match.group(1)
                        self.assignments[base_signal] = expression
                    else:
                        self.assignments[output_signal] = expression

    def _parse_instantiations(self, content: str):
        """Parse module instantiations."""
        # Pattern to match module instantiations like: module_name instance_name ( port connections );
        inst_pattern = r"(\w+)\s+(\w+)\s*\((.*?)\)\s*;"
        instantiations = re.findall(inst_pattern, content)

        for module_type, instance_name, connections in instantiations:
            # Skip if this looks like a module declaration
            if module_type == "module":
                continue

            # Parse port connections
            port_connections = {}
            # Pattern to match .port_name(signal_name) connections including bit selections like A[0], bus slices like A[3:0], and literals like 1'b0
            conn_pattern = r"\.([\w]+)\(([\w\[\]:']+)\)"
            connections_found = re.findall(conn_pattern, connections)

            for port_name, signal_name in connections_found:
                port_connections[port_name] = signal_name

            self.instantiations.append(
                {
                    "module_type": module_type,
                    "instance_name": instance_name,
                    "connections": port_connections,
                }
            )
    
    def _parse_sequential_blocks(self, content: str):
        """Parse sequential logic blocks like always_ff into executable AST."""
        pos = 0
        block_index = 0
        pattern = re.compile(r"always_ff\s*@\s*\(([^)]+)\)", re.IGNORECASE)

        while True:
            match = pattern.search(content, pos)
            if not match:
                break

            sensitivity_list = match.group(1).strip()
            clock_info = self._parse_sensitivity_list(sensitivity_list)
            body_start = self._skip_whitespace(content, match.end())

            if body_start >= len(content):
                break

            statement_ast = {"type": "block", "statements": []}
            next_pos = body_start

            if content.startswith("begin", body_start):
                begin_pos = body_start
                end_pos = self._find_matching_begin_end(content, begin_pos)
                block_body = content[begin_pos + len("begin"):end_pos].strip()
                statement_ast = self._parse_statement_block(block_body)
                next_pos = end_pos + len("end")
            else:
                statement_ast, next_pos = self._parse_statement(content, body_start)

            sequential_block = {
                "type": "always_ff",
                "clock": clock_info["clock"],
                "edge": clock_info["edge"],
                "statement": statement_ast,
                "order": block_index,
            }
            self.sequential_blocks.append(sequential_block)
            self.clock_signals.add(clock_info["clock"])
            block_index += 1
            pos = next_pos

    def _parse_combinational_blocks(self, content: str):
        """Parse always_comb blocks into executable AST."""
        pos = 0
        block_index = 0
        pattern = re.compile(r"\balways_comb\b")

        while True:
            match = pattern.search(content, pos)
            if not match:
                break

            body_start = self._skip_whitespace(content, match.end())
            if body_start >= len(content):
                break

            if content.startswith("begin", body_start):
                end_pos = self._find_matching_begin_end(content, body_start)
                block_body = content[body_start + len("begin"):end_pos].strip()
                statement_ast = self._parse_statement_block(block_body)
                next_pos = end_pos + len("end")
            else:
                statement_ast, next_pos = self._parse_statement(content, body_start)

            self.combinational_blocks.append({
                "type": "always_comb",
                "statement": statement_ast,
                "order": block_index,
            })
            block_index += 1
            pos = next_pos

    def _parse_sensitivity_list(self, sensitivity_list: str) -> Dict[str, str]:
        """Parse sensitivity list like 'posedge clk' or 'posedge clk or posedge rst'."""
        entries = [entry.strip() for entry in re.split(r"\bor\b|,", sensitivity_list) if entry.strip()]
        if not entries:
            return {"clock": "clk", "edge": "posedge"}

        first = entries[0]
        match = re.match(r"(posedge|negedge)\s+(\w+)", first)
        if match:
            return {"clock": match.group(2), "edge": match.group(1)}

        return {"clock": first, "edge": "posedge"}

    def _parse_statement_block(self, block_content: str) -> Dict[str, Any]:
        """Parse a begin/end block body into a list of statements."""
        statements = []
        pos = 0
        while True:
            pos = self._skip_whitespace(block_content, pos)
            if pos >= len(block_content):
                break
            statement, pos = self._parse_statement(block_content, pos)
            if statement and statement.get("type") != "empty":
                statements.append(statement)
        return {"type": "block", "statements": statements}

    def _parse_statement(self, text: str, pos: int) -> Tuple[Dict[str, Any], int]:
        pos = self._skip_whitespace(text, pos)
        if pos >= len(text):
            return {"type": "empty"}, pos

        if text.startswith("begin", pos):
            return self._parse_begin_block(text, pos)

        if text.startswith("if", pos) and self._is_keyword_boundary(text, pos, "if"):
            return self._parse_if_statement(text, pos)

        if text.startswith("case", pos) and self._is_keyword_boundary(text, pos, "case"):
            return self._parse_case_statement(text, pos)

        if text[pos] == ";":
            return {"type": "empty"}, pos + 1

        statement_text, next_pos = self._consume_until_semicolon(text, pos)
        parsed = self._parse_assignment_statement(statement_text)
        return parsed, next_pos

    def _parse_begin_block(self, text: str, pos: int) -> Tuple[Dict[str, Any], int]:
        begin_end = self._find_matching_begin_end(text, pos)
        inner = text[pos + len("begin"):begin_end]
        block_ast = self._parse_statement_block(inner)
        return block_ast, begin_end + len("end")

    def _parse_if_statement(self, text: str, pos: int) -> Tuple[Dict[str, Any], int]:
        cond_open = text.find("(", pos)
        if cond_open == -1:
            return {"type": "raw", "text": text[pos:].strip()}, len(text)
        condition, after_cond = self._extract_parenthesized(text, cond_open)
        then_stmt, cursor = self._parse_statement(text, after_cond)
        cursor = self._skip_whitespace(text, cursor)

        else_stmt = None
        if text.startswith("else", cursor) and self._is_keyword_boundary(text, cursor, "else"):
            else_stmt, cursor = self._parse_statement(text, cursor + len("else"))

        return {
            "type": "if",
            "condition": condition.strip(),
            "then": then_stmt,
            "else": else_stmt,
        }, cursor

    def _parse_case_statement(self, text: str, pos: int) -> Tuple[Dict[str, Any], int]:
        expr_open = text.find("(", pos)
        if expr_open == -1:
            return {"type": "raw", "text": text[pos:].strip()}, len(text)
        expression, cursor = self._extract_parenthesized(text, expr_open)

        items = []
        default_stmt = None

        while cursor < len(text):
            cursor = self._skip_whitespace(text, cursor)
            if text.startswith("endcase", cursor):
                cursor += len("endcase")
                break

            label_text, cursor = self._consume_until_colon(text, cursor)
            labels = [label.strip() for label in label_text.split(",") if label.strip()]
            stmt, cursor = self._parse_statement(text, cursor)

            if any(label == "default" for label in labels):
                default_stmt = stmt
            else:
                items.append({"labels": labels, "statement": stmt})

        return {
            "type": "case",
            "expression": expression.strip(),
            "items": items,
            "default": default_stmt,
        }, cursor

    def _parse_assignment_statement(self, statement_text: str) -> Dict[str, Any]:
        statement_text = statement_text.strip()
        if not statement_text:
            return {"type": "empty"}

        match = re.match(r"(.+?)(<=|=)(.+)", statement_text)
        if not match:
            return {"type": "raw", "text": statement_text}

        target_expr = match.group(1).strip()
        operator = match.group(2)
        rhs_expr = match.group(3).strip()
        target = self._parse_assignment_target(target_expr)

        if operator == "<=":
            return {"type": "nonblocking_assign", "target": target, "expression": rhs_expr}
        return {"type": "blocking_assign", "target": target, "expression": rhs_expr}

    def _parse_assignment_target(self, target_expr: str) -> Dict[str, Any]:
        mem_match = re.match(r"(\w+)\[(.+)\]$", target_expr)
        if mem_match:
            signal = mem_match.group(1)
            index_expr = mem_match.group(2).strip()
            if signal in self.memory_arrays:
                return {"kind": "memory", "memory": signal, "index": index_expr}

            if ":" in index_expr:
                slice_match = re.match(r"(\d+)\s*:\s*(\d+)$", index_expr)
                if slice_match:
                    return {
                        "kind": "slice",
                        "signal": signal,
                        "msb": int(slice_match.group(1)),
                        "lsb": int(slice_match.group(2)),
                    }

            bit_match = re.match(r"(\d+)$", index_expr)
            if bit_match:
                return {"kind": "bit", "signal": signal, "index": int(bit_match.group(1))}

            # Treat variable index on non-memory signals as generic index target.
            return {"kind": "indexed_signal", "signal": signal, "index": index_expr}

        return {"kind": "signal", "signal": target_expr}

    def _find_matching_begin_end(self, text: str, begin_pos: int) -> int:
        begin_count = 1
        pos = begin_pos + len("begin")

        while pos < len(text):
            begin_match = re.search(r"\bbegin\b", text[pos:])
            end_match = re.search(r"\bend\b", text[pos:])
            next_begin = pos + begin_match.start() if begin_match else float("inf")
            next_end = pos + end_match.start() if end_match else float("inf")

            if next_begin < next_end:
                begin_count += 1
                pos = next_begin + len("begin")
                continue

            if next_end != float("inf"):
                begin_count -= 1
                if begin_count == 0:
                    return next_end
                pos = next_end + len("end")
                continue

            break

        raise ValueError("Unmatched begin/end block")

    def _extract_parenthesized(self, text: str, open_paren_pos: int) -> Tuple[str, int]:
        depth = 0
        pos = open_paren_pos
        start = open_paren_pos + 1

        while pos < len(text):
            char = text[pos]
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    return text[start:pos], pos + 1
            pos += 1

        raise ValueError("Unmatched parentheses in sequential block")

    def _consume_until_delimiter(
        self, text: str, pos: int, delimiter: str, error_on_missing: bool = False
    ) -> Tuple[str, int]:
        """Consume text until a delimiter is found at zero nesting depth."""
        depth_paren = 0
        depth_bracket = 0
        depth_brace = 0
        start = pos

        while pos < len(text):
            char = text[pos]
            if char == "(":
                depth_paren += 1
            elif char == ")":
                depth_paren -= 1
            elif char == "[":
                depth_bracket += 1
            elif char == "]":
                depth_bracket -= 1
            elif char == "{":
                depth_brace += 1
            elif char == "}":
                depth_brace -= 1
            elif char == delimiter and depth_paren == 0 and depth_bracket == 0 and depth_brace == 0:
                return text[start:pos], pos + 1
            pos += 1

        if error_on_missing:
            raise ValueError(f"Malformed statement: missing '{delimiter}'")
        return text[start:].strip(), len(text)

    def _consume_until_semicolon(self, text: str, pos: int) -> Tuple[str, int]:
        return self._consume_until_delimiter(text, pos, ";", error_on_missing=False)

    def _consume_until_colon(self, text: str, pos: int) -> Tuple[str, int]:
        return self._consume_until_delimiter(text, pos, ":", error_on_missing=True)

    def _skip_whitespace(self, text: str, pos: int) -> int:
        while pos < len(text) and text[pos].isspace():
            pos += 1
        return pos

    def _is_keyword_boundary(self, text: str, start: int, keyword: str) -> bool:
        end = start + len(keyword)
        if not text.startswith(keyword, start):
            return False
        before_ok = start == 0 or not (text[start - 1].isalnum() or text[start - 1] == "_")
        after_ok = end >= len(text) or not (text[end].isalnum() or text[end] == "_")
        return before_ok and after_ok


class LogicEvaluator:
    """Evaluates SystemVerilog expressions with given input values."""

    def __init__(
        self,
        inputs: List[str],
        outputs: List[str],
        assignments: Dict[str, str],
        instantiations: List[Dict[str, Any]] = None,
        bus_info: Dict[str, Dict] = None,
        slice_assignments: List[Dict[str, Any]] = None,
        concat_assignments: List[Dict[str, Any]] = None,
        current_file_path: str = None,
        memory_arrays: Dict[str, Dict[str, Any]] = None,
        module_name: str = "",
        instance_path: str = "",
        memory_bindings: List[Dict[str, Any]] = None,
        combinational_blocks: List[Dict[str, Any]] = None,
    ):
        self.inputs = inputs
        self.outputs = outputs
        self.assignments = assignments
        self.slice_assignments = slice_assignments or []
        self.concat_assignments = concat_assignments or []
        self.instantiations = instantiations or []
        self.bus_info = bus_info or {}
        self.current_file_path = current_file_path
        self.module_name = module_name or "top_module"
        self.instance_path = instance_path
        self.memory_arrays = memory_arrays or {}
        self.memory_bindings = memory_bindings or []
        self.combinational_blocks = combinational_blocks or []
        self.memory_state: Dict[str, List[int]] = {}
        self.memory_access: Dict[str, str] = {}
        self.instance_evaluators: Dict[str, Any] = {}
        self.rom_data: Optional[List[int]] = None
        self.rom_addr_port: Optional[str] = None
        self.rom_data_port: Optional[str] = None
        self._last_signal_values: Dict[str, int] = {}

        self._initialize_memory_state()
        self._initialize_rom_primitive()

    def _initialize_rom_primitive(self):
        """Auto-detect rom_* modules and load ROM data by naming convention."""
        if not self.module_name.startswith("rom_"):
            return
        if not self.inputs or not self.outputs:
            return

        rom_name = self.module_name[4:]
        addr_port = self.inputs[0]
        data_port = self.outputs[0]
        addr_width = self.bus_info.get(addr_port, {}).get("width", 1)
        data_width = self.bus_info.get(data_port, {}).get("width", 1)
        depth = 1 << addr_width

        data_filename = f"{rom_name}.txt"
        search_dirs = self._rom_search_dirs()
        rom_file_path = next(
            (os.path.join(d, data_filename) for d in search_dirs
             if os.path.exists(os.path.join(d, data_filename))),
            None,
        )
        if rom_file_path is None:
            raise FileNotFoundError(
                f"ROM data file '{data_filename}' not found for module '{self.module_name}'. "
                f"Searched: {search_dirs}"
            )

        self.rom_data = load_memory_txt_file(rom_file_path, data_width, depth)
        self.rom_addr_port = addr_port
        self.rom_data_port = data_port

    def _rom_search_dirs(self) -> List[str]:
        """Return directories to search for ROM data files."""
        dirs: List[str] = []
        if self.current_file_path:
            sv_dir = os.path.dirname(self.current_file_path)
            dirs.append(sv_dir)
            dirs.append(os.path.join(sv_dir, "roms"))
        dirs.append(os.path.join(os.getcwd(), "roms"))
        return dirs

    def _initialize_memory_state(self):
        for memory_name, memory_info in self.memory_arrays.items():
            depth = memory_info.get("depth", 0)
            self.memory_state[memory_name] = [0] * depth
            self.memory_access[memory_name] = "ram"
        self._apply_memory_bindings()

    def _binding_matches_instance(self, binding_instance: str) -> bool:
        if not binding_instance:
            return True
        if self.instance_path == binding_instance:
            return True
        return self.instance_path.endswith(f".{binding_instance}")

    def _apply_memory_bindings(self):
        for binding in self.memory_bindings:
            binding_module = binding.get("module", "")
            binding_instance = binding.get("instance", "")
            memory_name = binding.get("memory", "")
            file_path = binding.get("file", "")
            memory_type = (binding.get("type") or "ram").lower()

            if binding_module and binding_module != self.module_name:
                continue
            if not self._binding_matches_instance(binding_instance):
                continue
            if memory_name and memory_name not in self.memory_arrays:
                raise ValueError(
                    f"Memory binding refers to unknown memory '{memory_name}' in module '{self.module_name}'"
                )
            if not file_path:
                raise ValueError(
                    f"Memory binding missing file path for module '{self.module_name}'"
                )
            if not os.path.exists(file_path):
                raise FileNotFoundError(
                    f"Memory init file not found: {file_path}"
                )

            if memory_name:
                memory_names = [memory_name]
            else:
                memory_names = list(self.memory_arrays.keys())

            for mem_name in memory_names:
                mem_info = self.memory_arrays.get(mem_name)
                if not mem_info:
                    continue
                loaded = load_memory_txt_file(
                    file_path,
                    mem_info.get("word_width", 1),
                    mem_info.get("depth", 0),
                )
                self.memory_state[mem_name] = loaded
                self.memory_access[mem_name] = memory_type

    def configure_memory_bindings(self, memory_bindings: List[Dict[str, Any]]):
        self.memory_bindings = memory_bindings or []
        self._initialize_memory_state()
        for evaluator in self.instance_evaluators.values():
            if hasattr(evaluator, "configure_memory_bindings"):
                evaluator.configure_memory_bindings(self.memory_bindings)

    def evaluate(
        self, input_values: Dict[str, int], advance_sequential_instances: bool = True
    ) -> Dict[str, int]:
        """
        Evaluate all output expressions for given input values.

        Args:
            input_values: Dictionary mapping input names to their values (buses as integers)

        Returns:
            Dictionary mapping output names to their computed values
        """
        # ROM primitive: simple address-to-data lookup
        if self.rom_data is not None:
            addr = input_values.get(self.rom_addr_port, 0) % len(self.rom_data)
            return {self.rom_data_port: self.rom_data[addr]}

        # Start with input values and expand buses to individual bits
        signal_values = {}
        for signal_name, value in input_values.items():
            signal_values[signal_name] = value
            # If this is a bus, also create individual bit signals
            if signal_name in self.bus_info:
                bus_info = self.bus_info[signal_name]
                if bus_info["width"] > 1:
                    self._expand_bus_to_bits(signal_name, value, signal_values)

        # Evaluate module instantiations first
        for inst in self.instantiations:
            self._evaluate_instantiation(
                inst, signal_values, advance_sequential_instances=advance_sequential_instances
            )

        # Evaluate all assignments (including intermediate wires) until no more changes
        # This handles cases where assignments depend on each other
        max_iterations = len(self.assignments) + len(self.combinational_blocks) * 2 + 10
        iteration = 0

        # Pre-compute comb block targets (static per AST, no need to recompute each iteration)
        comb_targets = [
            self._comb_block_targets(block) for block in self.combinational_blocks
        ]

        while iteration < max_iterations:
            changed = False
            iteration += 1

            for signal_name, expression in self.assignments.items():
                try:
                    new_value = self._evaluate_expression(
                        expression, signal_values, signal_name
                    )
                    if (
                        signal_name not in signal_values
                        or signal_values[signal_name] != new_value
                    ):
                        signal_values[signal_name] = new_value
                        changed = True
                        # If this is a bus, expand to individual bits
                        if (
                            signal_name in self.bus_info
                            and self.bus_info[signal_name]["width"] > 1
                        ):
                            self._expand_bus_to_bits(
                                signal_name, new_value, signal_values
                            )
                except Exception:
                    # Skip assignments that can't be evaluated yet (dependencies not ready)
                    continue

            # Execute always_comb blocks
            for comb_block, targets in zip(self.combinational_blocks, comb_targets):
                snapshot = {sig: signal_values.get(sig) for sig in targets}
                self._execute_comb_statement(
                    comb_block.get("statement", {}), signal_values
                )
                for sig in targets:
                    if signal_values.get(sig) != snapshot[sig]:
                        changed = True

            # If no changes were made, we're done
            if not changed:
                break

        # Process slice assignments after regular assignments
        for slice_assign in self.slice_assignments:
            signal_name = slice_assign["signal"]
            msb = slice_assign["msb"]
            lsb = slice_assign["lsb"]
            expression = slice_assign["expression"]

            # Evaluate the slice expression - determine the target width for the expression
            expr_width = width = abs(msb - lsb) + 1
            slice_value = self._evaluate_expression(expression, signal_values)
            # Mask to the expected width
            mask_expr = (1 << expr_width) - 1
            slice_value = slice_value & mask_expr

            # Initialize the target signal if it doesn't exist
            if signal_name not in signal_values:
                signal_values[signal_name] = 0

            # Update the specific slice of the bus
            width = abs(msb - lsb) + 1
            shift = lsb if msb >= lsb else msb
            mask = (1 << width) - 1

            # Clear the target bits and set the new value
            signal_values[signal_name] = (
                signal_values[signal_name] & ~(mask << shift)
            ) | ((slice_value & mask) << shift)

            # Also expand this updated bus to individual bits for consistency
            if signal_name in self.bus_info and self.bus_info[signal_name]["width"] > 1:
                self._expand_bus_to_bits(
                    signal_name, signal_values[signal_name], signal_values
                )

        # Process concatenation assignments after slice assignments
        for concat_assign in self.concat_assignments:
            targets = concat_assign["targets"]
            expression = concat_assign["expression"]

            # Evaluate the expression to get the combined value
            combined_value = self._evaluate_expression(expression, signal_values)

            # Calculate total width needed for all targets
            total_width = 0
            target_widths = []
            for target in targets:
                if target in self.bus_info:
                    width = self.bus_info[target]["width"]
                else:
                    width = 1  # Default to single bit
                target_widths.append(width)
                total_width += width

            # Split the combined value across targets (MSB first)
            current_value = combined_value
            for i, target in enumerate(
                reversed(targets)
            ):  # Process in reverse order (LSB first)
                width = target_widths[len(targets) - 1 - i]
                mask = (1 << width) - 1
                target_value = current_value & mask
                current_value >>= width

                signal_values[target] = target_value

                # Also expand this bus to individual bits for consistency
                if target in self.bus_info and self.bus_info[target]["width"] > 1:
                    self._expand_bus_to_bits(target, target_value, signal_values)

        # Preserve all signal values for parent sequential evaluators that need
        # access to intermediate combinational signals (e.g. structural CPU designs
        # where always_ff reads wires driven by sub-module instantiations).
        self._last_signal_values = signal_values

        # Extract output values
        output_values = {}
        for output_name in self.outputs:
            if output_name in signal_values:
                output_values[output_name] = signal_values[output_name]
            else:
                # Check if this is a bus that needs to be collected from individual bits
                if (
                    output_name in self.bus_info
                    and self.bus_info[output_name]["width"] > 1
                ):
                    bus_value = self._collect_bus_from_bits(output_name, signal_values)
                    output_values[output_name] = bus_value
                    signal_values[output_name] = bus_value

        return output_values

    def _execute_comb_statement(
        self, statement: Dict[str, Any], signal_values: Dict[str, int]
    ):
        """Execute a combinational statement AST node, updating signal_values in place."""
        stype = statement.get("type")

        if stype == "block":
            for child in statement.get("statements", []):
                self._execute_comb_statement(child, signal_values)
            return

        if stype == "if":
            condition = statement.get("condition", "0")
            cond_value = self._evaluate_expression(condition, signal_values)
            branch = statement.get("then") if cond_value else statement.get("else")
            if branch:
                self._execute_comb_statement(branch, signal_values)
            return

        if stype == "case":
            case_value = self._evaluate_expression(
                statement.get("expression", "0"), signal_values
            )
            selected = None
            for item in statement.get("items", []):
                for label in item.get("labels", []):
                    if self._evaluate_expression(label, signal_values) == case_value:
                        selected = item.get("statement")
                        break
                if selected:
                    break
            if not selected:
                selected = statement.get("default")
            if selected:
                self._execute_comb_statement(selected, signal_values)
            return

        if stype in {"blocking_assign", "nonblocking_assign"}:
            target = statement.get("target", {})
            target_signal = target.get("signal")
            value = self._evaluate_expression(
                statement.get("expression", "0"), signal_values, target_signal
            )
            self._apply_comb_assignment(target, value, signal_values)
            return

    def _apply_comb_assignment(
        self, target: Dict[str, Any], value: int, signal_values: Dict[str, int]
    ):
        """Apply a parsed assignment target to signal_values (combinational context)."""
        kind = target.get("kind")
        signal_name = target.get("signal")
        if not signal_name:
            return

        if kind == "bit":
            bit_index = int(target.get("index", 0))
            current = signal_values.get(signal_name, 0)
            if value & 1:
                signal_values[signal_name] = current | (1 << bit_index)
            else:
                signal_values[signal_name] = current & ~(1 << bit_index)
        elif kind == "slice":
            msb = int(target.get("msb", 0))
            lsb = int(target.get("lsb", 0))
            width = abs(msb - lsb) + 1
            shift = min(msb, lsb)
            mask = (1 << width) - 1
            current = signal_values.get(signal_name, 0)
            signal_values[signal_name] = (current & ~(mask << shift)) | ((value & mask) << shift)
        else:
            width = self.bus_info.get(signal_name, {}).get("width", 1)
            signal_values[signal_name] = value & ((1 << width) - 1)

        # Expand bus bits for consistency
        if signal_name in self.bus_info and self.bus_info[signal_name].get("width", 1) > 1:
            self._expand_bus_to_bits(signal_name, signal_values[signal_name], signal_values)

    def _comb_block_targets(self, block: Dict[str, Any]) -> set:
        """Collect all target signal names from a combinational block's AST."""
        targets: set = set()

        def collect(stmt: Dict[str, Any]):
            stype = stmt.get("type")
            if stype in {"blocking_assign", "nonblocking_assign"}:
                sig = stmt.get("target", {}).get("signal")
                if sig:
                    targets.add(sig)
            elif stype == "block":
                for child in stmt.get("statements", []):
                    collect(child)
            elif stype == "if":
                if stmt.get("then"):
                    collect(stmt["then"])
                if stmt.get("else"):
                    collect(stmt["else"])
            elif stype == "case":
                for item in stmt.get("items", []):
                    collect(item.get("statement", {}))
                if stmt.get("default"):
                    collect(stmt["default"])

        collect(block.get("statement", {}))
        return targets

    def _evaluate_expression(
        self, expression: str, signal_values: Dict[str, int], target_signal: str = None
    ) -> int:
        """Evaluate a single SystemVerilog expression."""
        eval_expr = expression.strip()

        # Handle simple bus-to-bus assignment (like Y = A)
        if eval_expr in signal_values:
            return signal_values[eval_expr]

        # Handle concatenation expressions like {a, b, c}
        if eval_expr.startswith("{") and eval_expr.endswith("}"):
            return self._evaluate_concatenation(eval_expr, signal_values)

        # Handle memory reads like mem[address]
        memory_access_pattern = r"(\w+)\[([^\[\]:]+)\]"

        def replace_memory_access(match):
            memory_name = match.group(1)
            index_expr = match.group(2).strip()
            if memory_name not in self.memory_state:
                return match.group(0)
            try:
                index_value = self._evaluate_expression(index_expr, signal_values)
            except Exception:
                index_value = 0
            memory_data = self.memory_state.get(memory_name, [])
            if not memory_data:
                return "0"
            index_value = max(0, min(len(memory_data) - 1, int(index_value)))
            return str(memory_data[index_value])

        eval_expr = re.sub(memory_access_pattern, replace_memory_access, eval_expr)

        # Handle bus slice expressions like A[7:0], in[15:8] first
        bus_slice_pattern = r"(\w+)\[(\d+):(\d+)\]"

        def replace_bus_slice(match):
            bus_name = match.group(1)
            msb = int(match.group(2))
            lsb = int(match.group(3))

            # Extract the bus value
            if bus_name in signal_values:
                bus_value = signal_values[bus_name]
                # Extract the specified bits
                width = abs(msb - lsb) + 1
                shift = lsb if msb >= lsb else msb
                mask = (1 << width) - 1
                slice_value = (bus_value >> shift) & mask
                return str(slice_value)
            return match.group(0)  # Return original if not found

        eval_expr = re.sub(bus_slice_pattern, replace_bus_slice, eval_expr)

        # Handle single bus bit selection like A[2], B[0]
        bit_select_pattern = r"(\w+)\[(\d+)\]"

        def replace_bit_select(match):
            bus_name = match.group(1)
            bit_index = int(match.group(2))
            bit_signal = f"{bus_name}[{bit_index}]"
            if bit_signal in signal_values:
                return str(signal_values[bit_signal])
            return match.group(0)  # Return original if not found

        eval_expr = re.sub(bit_select_pattern, replace_bit_select, eval_expr)

        # Replace SystemVerilog literal constants
        literal_pattern = r"(\d+)'([bhdBHD])([0-9a-fA-F_xXzZ]+)"

        def replace_literal(match):
            width = int(match.group(1))
            base = match.group(2).lower()
            value_str = match.group(3).replace("_", "").lower().replace("x", "0").replace("z", "0")
            if base == "b":
                value = int(value_str, 2)
            elif base == "h":
                value = int(value_str, 16)
            else:
                value = int(value_str, 10)
            return str(value & ((1 << width) - 1))

        eval_expr = re.sub(literal_pattern, replace_literal, eval_expr)

        # Replace identifiers with current values
        for signal_name, value in signal_values.items():
            if "[" not in signal_name:
                eval_expr = re.sub(
                    r"\b" + re.escape(signal_name) + r"\b", str(value), eval_expr
                )

        # Convert ternary operator (after slices/literals are resolved, so : is unambiguous)
        if "?" in eval_expr:
            eval_expr = self._convert_ternary(eval_expr)

        # Convert SystemVerilog operators to Python equivalents
        eval_expr = self._convert_operators(eval_expr)

        try:
            result = eval(eval_expr, {"__builtins__": {}}, {})
            result = int(result)

            # Apply bit masking based on target signal width
            if target_signal:
                if target_signal in self.bus_info:
                    width = self.bus_info[target_signal].get("width", 1)
                    if width > 1:
                        return result & ((1 << width) - 1)
                return result & 1

            # Expressions without explicit assignment target keep full width
            return result
        except Exception as e:
            raise ValueError(f"Error evaluating expression '{expression}': {e}")

    def _convert_operators(self, expression: str) -> str:
        """Convert SystemVerilog operators to Python equivalents."""
        # Logical operators
        expression = expression.replace("&&", " and ")
        expression = expression.replace("||", " or ")
        expression = re.sub(r"(?<![=!<>])!(?!=)", " not ", expression)

        return expression

    def _convert_ternary(self, expression: str) -> str:
        """Convert SV ternary `cond ? true_val : false_val` to Python `((true_val) if (cond) else (false_val))`."""
        # Process right-to-left (ternary is right-associative in SV)
        q_pos = expression.rfind("?")
        if q_pos == -1:
            return expression
        cond = expression[:q_pos].strip()
        rest = expression[q_pos + 1:].strip()
        colon_pos = self._find_ternary_colon(rest)
        if colon_pos == -1:
            return expression
        true_val = rest[:colon_pos].strip()
        false_val = rest[colon_pos + 1:].strip()
        # Recursively handle nested ternaries in the condition part
        if "?" in cond:
            cond = self._convert_ternary(cond)
        if "?" in false_val:
            false_val = self._convert_ternary(false_val)
        if "?" in true_val:
            true_val = self._convert_ternary(true_val)
        return f"(({true_val}) if ({cond}) else ({false_val}))"

    def _find_ternary_colon(self, text: str) -> int:
        """Find the colon matching a ternary '?' while respecting paren/bracket nesting."""
        depth = 0
        for i, ch in enumerate(text):
            if ch in "({[":
                depth += 1
            elif ch in ")}]":
                depth -= 1
            elif ch == ":" and depth == 0:
                return i
        return -1

    def _evaluate_concatenation(
        self, concat_expr: str, signal_values: Dict[str, int]
    ) -> int:
        """Evaluate concatenation expressions like {a, b, c} and replication like {N{expr}}."""
        # Remove curly braces
        inner_expr = concat_expr[1:-1].strip()

        # Split by comma and evaluate each part
        parts = [part.strip() for part in inner_expr.split(",")]

        result = 0
        for part in parts:
            part_width = 1  # Default width
            replication_match = None  # Initialize for each part

            # Check if this part is itself a concatenation (nested braces)
            if part.startswith("{") and part.endswith("}"):
                # Recursively evaluate nested concatenation
                part_value = self._evaluate_concatenation(part, signal_values)
                # For nested concatenations, we need to calculate the actual width
                # by analyzing the inner content
                part_width = self._calculate_concatenation_width(part, signal_values)

            # Check for replication pattern like {N{expression}}
            elif replication_match := re.match(r"(\d+)\{(.+?)\}", part):
                # Handle replication: N{expression}
                count = int(replication_match.group(1))
                expr = replication_match.group(2).strip()

                # Evaluate the replicated expression
                replicated_value = self._evaluate_expression(expr, signal_values)

                # Determine the width of the replicated expression
                # For single bit expressions, width is 1
                expr_width = 1  # Default for single bits like in[7]
                if expr in self.bus_info:
                    expr_width = self.bus_info[expr]["width"]
                elif re.match(r"\w+\[\d+:\d+\]", expr):
                    # Handle bus slice width calculation
                    slice_match = re.match(r"\w+\[(\d+):(\d+)\]", expr)
                    if slice_match:
                        msb, lsb = int(slice_match.group(1)), int(slice_match.group(2))
                        expr_width = abs(msb - lsb) + 1

                # Create the replicated bits
                part_value = 0
                for i in range(count):
                    part_value = (part_value << expr_width) | (
                        replicated_value & ((1 << expr_width) - 1)
                    )

                part_width = count * expr_width

            # Evaluate each part of the concatenation
            elif part in signal_values:
                part_value = signal_values[part]
            else:
                # Handle literal constants like 2'b11, 4'hF, 8'd255
                literal_match = re.match(r"(\d+)'([bhdBHD])([0-9a-fA-F]+)", part)
                if literal_match:
                    width = int(literal_match.group(1))
                    base = literal_match.group(2).lower()
                    value_str = literal_match.group(3)

                    if base == "b":  # Binary
                        part_value = int(value_str, 2)
                    elif base == "h":  # Hexadecimal
                        part_value = int(value_str, 16)
                    elif base == "d":  # Decimal
                        part_value = int(value_str, 10)
                    else:
                        part_value = 0

                    # Mask to specified width
                    part_value &= (1 << width) - 1
                # Handle bit selections like in[0], in[1], etc.
                else:
                    bit_select_match = re.match(r"(\w+)\[(\d+)\]", part)
                    if bit_select_match:
                        bus_name = bit_select_match.group(1)
                        bit_index = int(bit_select_match.group(2))
                        bit_signal = f"{bus_name}[{bit_index}]"
                        if bit_signal in signal_values:
                            part_value = signal_values[bit_signal]
                        else:
                            # Extract from bus value
                            if bus_name in signal_values:
                                bus_value = signal_values[bus_name]
                                part_value = (bus_value >> bit_index) & 1
                            else:
                                part_value = 0
                    else:
                        part_value = 0

            # Determine the width of this part (if not already set by replication logic)
            if not replication_match:
                if part in signal_values and part in self.bus_info:
                    part_width = self.bus_info[part]["width"]
                elif re.match(r"(\d+)'[bhdBHD]", part):  # Literal constant
                    literal_match = re.match(r"(\d+)'[bhdBHD]", part)
                    part_width = int(literal_match.group(1))
                # part_width already defaults to 1

            # Shift previous results and add this part (MSB first)
            result = (result << part_width) | (part_value & ((1 << part_width) - 1))

        return result

    def _calculate_concatenation_width(
        self, concat_expr: str, signal_values: Dict[str, int]
    ) -> int:
        """Calculate the total bit width of a concatenation expression."""
        # Remove curly braces
        inner_expr = concat_expr[1:-1].strip()

        # Split by comma and calculate width of each part
        parts = [part.strip() for part in inner_expr.split(",")]

        total_width = 0
        for part in parts:
            # Check for replication pattern like {N{expression}}
            replication_match = re.match(r"(\d+)\{(.+?)\}", part)
            if replication_match:
                count = int(replication_match.group(1))
                expr = replication_match.group(2).strip()

                # Determine width of replicated expression
                expr_width = 1  # Default for single bits like in[7]
                if expr in self.bus_info:
                    expr_width = self.bus_info[expr]["width"]
                elif re.match(r"\w+\[\d+:\d+\]", expr):
                    slice_match = re.match(r"\w+\[(\d+):(\d+)\]", expr)
                    if slice_match:
                        msb, lsb = int(slice_match.group(1)), int(slice_match.group(2))
                        expr_width = abs(msb - lsb) + 1

                total_width += count * expr_width

            elif part in signal_values and part in self.bus_info:
                total_width += self.bus_info[part]["width"]
            elif re.match(r"(\d+)'[bhdBHD]", part):  # Literal constant
                literal_match = re.match(r"(\d+)'[bhdBHD]", part)
                total_width += int(literal_match.group(1))
            else:
                total_width += 1  # Default single bit

        return total_width

    def _evaluate_instantiation(
        self,
        inst: Dict[str, Any],
        signal_values: Dict[str, int],
        advance_sequential_instances: bool = True,
    ):
        """Evaluate a module instantiation with persistent per-instance state."""
        module_type = inst["module_type"]
        connections = inst["connections"]
        instance_name = inst.get("instance_name", "unknown")
        instance_path = (
            f"{self.instance_path}.{instance_name}" if self.instance_path else instance_name
        )

        if module_type not in GLOBAL_MODULE_CACHE:
            self._load_module(module_type)
        if module_type not in GLOBAL_MODULE_CACHE:
            raise ValueError(f"Could not load module '{module_type}' for instance '{instance_name}'")

        module_info = GLOBAL_MODULE_CACHE[module_type]

        inst_input_values = {}
        for port_name, signal_name in connections.items():
            if port_name not in module_info["inputs"]:
                continue
            signal_value = self._resolve_signal_reference(signal_name, signal_values)
            if signal_value is None:
                available_signals = list(signal_values.keys())
                raise ValueError(
                    f"Signal '{signal_name}' not found for instantiation '{instance_name}' "
                    f"(port '{port_name}'). Available signals: "
                    f"{available_signals[:10]}{'...' if len(available_signals) > 10 else ''}"
                )
            inst_input_values[port_name] = signal_value

        if instance_name not in self.instance_evaluators:
            self.instance_evaluators[instance_name] = create_evaluator(
                module_info,
                filepath=module_info.get("filepath", self.current_file_path),
                module_name=module_info.get("name", module_type),
                instance_path=instance_path,
                memory_bindings=self.memory_bindings,
            )

        inst_evaluator = self.instance_evaluators[instance_name]
        if hasattr(inst_evaluator, "evaluate_cycle"):
            if advance_sequential_instances:
                inst_outputs = inst_evaluator.evaluate_cycle(inst_input_values)
            elif hasattr(inst_evaluator, "peek_outputs"):
                inst_outputs = inst_evaluator.peek_outputs(inst_input_values)
            else:
                inst_outputs = inst_evaluator.evaluate(inst_input_values)
        else:
            inst_outputs = inst_evaluator.evaluate(inst_input_values)

        # Map outputs back to the parent module's signals
        for port_name, signal_name in connections.items():
            if port_name in module_info["outputs"]:
                # Check if it's a bus slice assignment like outSum[3:0]
                bus_slice_match = re.match(r"(\w+)\[(\d+):(\d+)\]", signal_name)
                if bus_slice_match:
                    bus_name = bus_slice_match.group(1)
                    msb = int(bus_slice_match.group(2))
                    lsb = int(bus_slice_match.group(3))
                    output_value = inst_outputs[port_name]

                    # Initialize bus if not exists
                    if bus_name not in signal_values:
                        signal_values[bus_name] = 0

                    # Update the specific slice of the bus
                    width = abs(msb - lsb) + 1
                    shift = lsb if msb >= lsb else msb
                    mask = (1 << width) - 1

                    # Clear the target bits and set new value
                    signal_values[bus_name] = (
                        signal_values[bus_name] & ~(mask << shift)
                    ) | ((output_value & mask) << shift)

                    # Also expand this updated bus to individual bits for consistency
                    if (
                        bus_name in self.bus_info
                        and self.bus_info[bus_name]["width"] > 1
                    ):
                        self._expand_bus_to_bits(
                            bus_name, signal_values[bus_name], signal_values
                        )
                else:
                    # Handle direct signal assignment
                    signal_values[signal_name] = inst_outputs[port_name]

                    # Also handle bit selection assignment like Sum[0]
                    bit_select_match = re.match(r"(\w+)\[(\d+)\]", signal_name)
                    if bit_select_match:
                        bus_name = bit_select_match.group(1)
                        bit_index = int(bit_select_match.group(2))
                        bit_signal_name = f"{bus_name}[{bit_index}]"
                        signal_values[bit_signal_name] = inst_outputs[port_name]

    def _resolve_signal_reference(self, signal_name: str, signal_values: Dict[str, int]) -> Optional[int]:
        """Resolve a connected signal/expression from parent scope."""
        signal_name = signal_name.strip()

        # SystemVerilog literal
        literal_match = re.match(r"(\d+)'([bhdBHD])([0-9a-fA-F_xXzZ]+)$", signal_name)
        if literal_match:
            width = int(literal_match.group(1))
            base = literal_match.group(2).lower()
            value_str = literal_match.group(3).replace("_", "").lower().replace("x", "0").replace("z", "0")
            if base == "b":
                value = int(value_str, 2)
            elif base == "h":
                value = int(value_str, 16)
            else:
                value = int(value_str, 10)
            return value & ((1 << width) - 1)

        if signal_name in signal_values:
            return signal_values[signal_name]

        # Bus slice
        bus_slice_match = re.match(r"(\w+)\[(\d+):(\d+)\]$", signal_name)
        if bus_slice_match:
            bus_name = bus_slice_match.group(1)
            msb = int(bus_slice_match.group(2))
            lsb = int(bus_slice_match.group(3))
            if bus_name in signal_values:
                bus_value = signal_values[bus_name]
                width = abs(msb - lsb) + 1
                shift = lsb if msb >= lsb else msb
                mask = (1 << width) - 1
                return (bus_value >> shift) & mask

        # Bit select
        bit_select_match = re.match(r"(\w+)\[(\d+)\]$", signal_name)
        if bit_select_match:
            bus_name = bit_select_match.group(1)
            bit_index = int(bit_select_match.group(2))
            bit_signal_name = f"{bus_name}[{bit_index}]"
            if bit_signal_name in signal_values:
                return signal_values[bit_signal_name]
            if bus_name in signal_values:
                return (signal_values[bus_name] >> bit_index) & 1

        # Numeric literal without width
        if re.fullmatch(r"\d+", signal_name):
            return int(signal_name)

        return None

    def _expand_bus_to_bits(
        self, bus_name: str, bus_value: int, signal_values: Dict[str, int]
    ):
        """Expand a bus value into individual bit signals."""
        bus_info = self.bus_info[bus_name]
        msb, lsb = bus_info["msb"], bus_info["lsb"]

        # Create individual bit signals like A[3], A[2], A[1], A[0] for a 4-bit bus
        for i in range(max(msb, lsb), min(msb, lsb) - 1, -1):
            bit_index = abs(i - lsb) if msb >= lsb else abs(lsb - i)
            bit_value = (bus_value >> bit_index) & 1
            signal_values[f"{bus_name}[{i}]"] = bit_value

    def _collect_bus_from_bits(
        self, bus_name: str, signal_values: Dict[str, int]
    ) -> int:
        """Collect individual bit signals back into a bus value."""
        bus_info = self.bus_info[bus_name]
        msb, lsb = bus_info["msb"], bus_info["lsb"]
        bus_value = 0

        for i in range(max(msb, lsb), min(msb, lsb) - 1, -1):
            bit_index = abs(i - lsb) if msb >= lsb else abs(lsb - i)
            bit_name = f"{bus_name}[{i}]"
            if bit_name in signal_values:
                bus_value |= (signal_values[bit_name] & 1) << bit_index

        return bus_value

    def reset_instance_state(self):
        """Reset cached sub-module instance state."""
        for evaluator in self.instance_evaluators.values():
            if hasattr(evaluator, "reset_state"):
                evaluator.reset_state()
            elif hasattr(evaluator, "reset_instance_state"):
                evaluator.reset_instance_state()

    def count_nand_gates(self) -> int:
        """Count the total number of NAND gates in the module hierarchy."""
        return self._count_nand_gates_recursive("top_module", set())

    def _count_nand_gates_recursive(self, module_name: str, visited: set) -> int:
        """Recursively count NAND gates in a module and its sub-modules."""
        # Avoid infinite recursion
        if module_name in visited:
            return 0
        visited.add(module_name)

        # Check if this is the primitive NAND gate
        if module_name == "nand_gate":
            return 1

        # Load module if not already loaded
        if module_name not in GLOBAL_MODULE_CACHE:
            # For the top module, use current module info
            if module_name == "top_module":
                # Create a temporary module info from current evaluator
                temp_module_info = {
                    "name": "top_module",
                    "instantiations": self.instantiations,
                }
                GLOBAL_MODULE_CACHE[module_name] = temp_module_info
            else:
                self._load_module(module_name)

        if module_name not in GLOBAL_MODULE_CACHE:
            return 0

        module_info = GLOBAL_MODULE_CACHE[module_name]
        total_nands = 0

        # Count NAND gates in all instantiated sub-modules
        for inst in module_info.get("instantiations", []):
            sub_module_type = inst["module_type"]
            sub_nands = self._count_nand_gates_recursive(
                sub_module_type, visited.copy()
            )
            total_nands += sub_nands

        return total_nands

    def _module_search_paths(self, module_name: str) -> List[str]:
        """Return candidate paths for resolving an instantiated module."""
        search_paths = [f"{module_name}.sv"]
        if self.current_file_path:
            search_paths.append(
                os.path.join(os.path.dirname(self.current_file_path), f"{module_name}.sv")
            )

        ordered_paths: List[str] = []
        seen_paths = set()
        for path in search_paths:
            normalized = os.path.abspath(path)
            if normalized in seen_paths:
                continue
            seen_paths.add(normalized)
            ordered_paths.append(normalized)
        return ordered_paths

    def _load_module(self, module_name: str):
        """Load a module from disk using the current source file as context."""
        search_paths = self._module_search_paths(module_name)
        module_file = next((path for path in search_paths if os.path.exists(path)), None)

        if module_file is None:
            print(
                f"Warning: Module '{module_name}' not found. Searched: {search_paths}"
            )
            return

        try:
            parser = SystemVerilogParser()
            module_info = parser.parse_file(module_file)
            GLOBAL_MODULE_CACHE[module_name] = module_info
        except Exception as e:
            print(f"Warning: Could not load module '{module_name}': {e}")


class SequentialLogicEvaluator:
    """Evaluator for sequential (clocked) SystemVerilog modules."""
    def __init__(
        self,
        inputs: List[str],
        outputs: List[str],
        assignments: Dict[str, str],
        instantiations: List[Dict],
        bus_info: Dict[str, Dict],
        slice_assignments: List[Dict],
        concat_assignments: List[Dict],
        sequential_blocks: List[Dict],
        clock_signals: List[str],
        filepath: str = "",
        memory_arrays: Dict[str, Dict[str, Any]] = None,
        module_name: str = "",
        instance_path: str = "",
        memory_bindings: List[Dict[str, Any]] = None,
        combinational_blocks: List[Dict[str, Any]] = None,
    ):
        self.inputs = inputs
        self.outputs = outputs
        self.sequential_blocks = sorted(sequential_blocks or [], key=lambda b: b.get("order", 0))
        self.clock_signals = set(clock_signals or [])
        self.bus_info = bus_info or {}
        self.module_name = module_name or "top_module"
        self.instance_path = instance_path

        self.comb_evaluator = LogicEvaluator(
            inputs,
            outputs,
            assignments,
            instantiations,
            bus_info,
            slice_assignments,
            concat_assignments,
            filepath,
            memory_arrays or {},
            self.module_name,
            self.instance_path,
            memory_bindings or [],
            combinational_blocks or [],
        )
        self.memory_arrays = self.comb_evaluator.memory_arrays

        self.state_signals = self._collect_state_signals()
        self.state: Dict[str, int] = {}
        self.reset_state()

    def _collect_state_signals(self) -> set:
        signals = set(self.outputs)

        def collect_from_statement(statement: Dict[str, Any]):
            stype = statement.get("type")
            if stype in {"nonblocking_assign", "blocking_assign"}:
                target = statement.get("target", {})
                kind = target.get("kind")
                if kind in {"signal", "bit", "slice", "indexed_signal"}:
                    signals.add(target.get("signal"))
            elif stype == "block":
                for child in statement.get("statements", []):
                    collect_from_statement(child)
            elif stype == "if":
                if statement.get("then"):
                    collect_from_statement(statement["then"])
                if statement.get("else"):
                    collect_from_statement(statement["else"])
            elif stype == "case":
                for item in statement.get("items", []):
                    collect_from_statement(item.get("statement", {}))
                if statement.get("default"):
                    collect_from_statement(statement["default"])

        for block in self.sequential_blocks:
            collect_from_statement(block.get("statement", {}))

        return {signal for signal in signals if signal}

    def _expand_known_buses(self, signal_values: Dict[str, int]):
        for signal_name, value in list(signal_values.items()):
            if (
                signal_name in self.bus_info
                and self.bus_info[signal_name].get("width", 1) > 1
                and "[" not in signal_name
            ):
                self.comb_evaluator._expand_bus_to_bits(signal_name, value, signal_values)

    def _clock_edge_active(self, block: Dict[str, Any], input_values: Dict[str, int]) -> bool:
        clock = block.get("clock")
        edge = block.get("edge", "posedge")
        if not clock or clock not in input_values:
            return True
        clock_value = input_values.get(clock, 0)
        if edge == "negedge":
            return clock_value == 0
        return clock_value == 1

    def evaluate_cycle(self, input_values: Dict[str, int]) -> Dict[str, int]:
        """Evaluate one clock cycle with nonblocking scheduling semantics."""
        current_signals = {**self.state, **input_values}
        self._expand_known_buses(current_signals)

        comb_outputs = self.comb_evaluator.evaluate(
            current_signals, advance_sequential_instances=True
        )
        # Use all combinational signal values (not just outputs) so that
        # always_ff blocks can read intermediate wires from sub-module
        # instantiations and assigns (needed for structural designs).
        all_comb_signals = self.comb_evaluator._last_signal_values
        snapshot = {**current_signals, **all_comb_signals}
        self._expand_known_buses(snapshot)

        blocking_updates: Dict[str, int] = {}
        nonblocking_updates: Dict[str, int] = {}
        blocking_mem_updates: Dict[Tuple[str, int], int] = {}
        nonblocking_mem_updates: Dict[Tuple[str, int], int] = {}

        for block in self.sequential_blocks:
            if block.get("type") != "always_ff":
                continue
            if not self._clock_edge_active(block, input_values):
                continue

            local_context = snapshot.copy()
            self._execute_statement(
                block.get("statement", {}),
                local_context,
                blocking_updates,
                nonblocking_updates,
                blocking_mem_updates,
                nonblocking_mem_updates,
            )

        next_state = self.state.copy()
        self._commit_updates(next_state, blocking_updates)
        self._commit_memory_updates(blocking_mem_updates)
        self._commit_updates(next_state, nonblocking_updates)
        self._commit_memory_updates(nonblocking_mem_updates)
        self.state = next_state

        post_signals = {**self.state, **input_values}
        self._expand_known_buses(post_signals)
        post_outputs = self.comb_evaluator.evaluate(
            post_signals, advance_sequential_instances=False
        )

        output_values: Dict[str, int] = {}
        for output in self.outputs:
            if output in post_outputs:
                output_values[output] = post_outputs[output]
            elif output in self.state:
                output_values[output] = self.state[output]
            elif output in self.bus_info and self.bus_info[output].get("width", 1) > 1:
                output_values[output] = self.comb_evaluator._collect_bus_from_bits(output, post_signals)
            else:
                output_values[output] = 0

        return output_values

    def _execute_statement(
        self,
        statement: Dict[str, Any],
        local_context: Dict[str, int],
        blocking_updates: Dict[str, int],
        nonblocking_updates: Dict[str, int],
        blocking_mem_updates: Dict[Tuple[str, int], int],
        nonblocking_mem_updates: Dict[Tuple[str, int], int],
    ):
        stype = statement.get("type")

        if stype == "block":
            for child in statement.get("statements", []):
                self._execute_statement(
                    child,
                    local_context,
                    blocking_updates,
                    nonblocking_updates,
                    blocking_mem_updates,
                    nonblocking_mem_updates,
                )
            return

        if stype == "if":
            condition = statement.get("condition", "0")
            cond_value = self.comb_evaluator._evaluate_expression(condition, local_context)
            branch = statement.get("then") if cond_value else statement.get("else")
            if branch:
                self._execute_statement(
                    branch,
                    local_context,
                    blocking_updates,
                    nonblocking_updates,
                    blocking_mem_updates,
                    nonblocking_mem_updates,
                )
            return

        if stype == "case":
            case_value = self.comb_evaluator._evaluate_expression(statement.get("expression", "0"), local_context)
            selected = None
            for item in statement.get("items", []):
                labels = item.get("labels", [])
                for label in labels:
                    label_value = self.comb_evaluator._evaluate_expression(label, local_context)
                    if label_value == case_value:
                        selected = item.get("statement")
                        break
                if selected:
                    break
            if not selected:
                selected = statement.get("default")
            if selected:
                self._execute_statement(
                    selected,
                    local_context,
                    blocking_updates,
                    nonblocking_updates,
                    blocking_mem_updates,
                    nonblocking_mem_updates,
                )
            return

        if stype in {"blocking_assign", "nonblocking_assign"}:
            target = statement.get("target", {})
            target_signal = target.get("signal")
            value = self.comb_evaluator._evaluate_expression(
                statement.get("expression", "0"),
                local_context,
                target_signal,
            )

            if stype == "blocking_assign":
                self._record_assignment(blocking_updates, blocking_mem_updates, target, value, local_context)
                self._apply_to_context(local_context, target, value)
            else:
                self._record_assignment(nonblocking_updates, nonblocking_mem_updates, target, value, local_context)
            return

    def _record_assignment(
        self,
        signal_updates: Dict[str, int],
        memory_updates: Dict[Tuple[str, int], int],
        target: Dict[str, Any],
        value: int,
        context: Dict[str, int],
    ):
        kind = target.get("kind")
        if kind == "memory":
            memory_name = target.get("memory")
            index_expr = target.get("index", "0")
            index_value = self.comb_evaluator._evaluate_expression(index_expr, context)
            memory_updates[(memory_name, int(index_value))] = value
            return

        signal_name = target.get("signal")
        if not signal_name:
            return
        signal_updates[signal_name] = self._apply_target_transform(signal_name, target, value, signal_updates.get(signal_name))

    def _apply_target_transform(
        self,
        signal_name: str,
        target: Dict[str, Any],
        value: int,
        existing_signal_value: Optional[int] = None,
    ) -> int:
        kind = target.get("kind")
        current_value = existing_signal_value
        if current_value is None:
            current_value = self.state.get(signal_name, 0)

        if kind == "bit":
            bit_index = int(target.get("index", 0))
            if value & 1:
                return current_value | (1 << bit_index)
            return current_value & ~(1 << bit_index)

        if kind == "slice":
            msb = int(target.get("msb", 0))
            lsb = int(target.get("lsb", 0))
            width = abs(msb - lsb) + 1
            shift = min(msb, lsb)
            mask = (1 << width) - 1
            return (current_value & ~(mask << shift)) | ((value & mask) << shift)

        if signal_name in self.bus_info:
            width = self.bus_info[signal_name].get("width", 1)
            if width > 1:
                return value & ((1 << width) - 1)
        return value & 1

    def _apply_to_context(self, context: Dict[str, int], target: Dict[str, Any], value: int):
        kind = target.get("kind")
        if kind == "memory":
            return
        signal_name = target.get("signal")
        if not signal_name:
            return
        updated = self._apply_target_transform(signal_name, target, value, context.get(signal_name))
        context[signal_name] = updated
        if signal_name in self.bus_info and self.bus_info[signal_name].get("width", 1) > 1:
            self.comb_evaluator._expand_bus_to_bits(signal_name, updated, context)
        elif kind == "bit":
            bit_index = int(target.get("index", 0))
            context[f"{signal_name}[{bit_index}]"] = value & 1

    def _commit_updates(self, next_state: Dict[str, int], updates: Dict[str, int]):
        for signal_name, value in updates.items():
            next_state[signal_name] = value

    def _commit_memory_updates(self, memory_updates: Dict[Tuple[str, int], int]):
        for (memory_name, index), value in memory_updates.items():
            if memory_name not in self.comb_evaluator.memory_state:
                continue
            if self.comb_evaluator.memory_access.get(memory_name) == "rom":
                continue
            mem_data = self.comb_evaluator.memory_state[memory_name]
            if index < 0 or index >= len(mem_data):
                continue
            word_width = self.memory_arrays.get(memory_name, {}).get("word_width", 1)
            mem_data[index] = value & ((1 << word_width) - 1)

    def configure_memory_bindings(self, memory_bindings: List[Dict[str, Any]]):
        self.comb_evaluator.configure_memory_bindings(memory_bindings)
        self.reset_state()

    def reset_state(self):
        """Reset sequential signal state and nested instance state."""
        self.state = {signal_name: 0 for signal_name in self.state_signals}
        self.comb_evaluator._initialize_memory_state()
        self.comb_evaluator.reset_instance_state()

    def count_nand_gates(self) -> int:
        """Count NAND gates by delegating to the combinational evaluator."""
        return self.comb_evaluator.count_nand_gates()

    def evaluate(self, input_values: Dict[str, int]) -> Dict[str, int]:
        """Compatibility wrapper - evaluates one cycle for truth table generation."""
        return self.evaluate_cycle(input_values)

    def peek_outputs(self, input_values: Dict[str, int]) -> Dict[str, int]:
        """Read current outputs without advancing sequential state."""
        signals = {**self.state, **input_values}
        self._expand_known_buses(signals)
        comb_outputs = self.comb_evaluator.evaluate(
            signals, advance_sequential_instances=False
        )
        result = {}
        for output in self.outputs:
            if output in comb_outputs:
                result[output] = comb_outputs[output]
            elif output in self.state:
                result[output] = self.state[output]
            else:
                result[output] = 0
        return result


def _has_sequential_submodules(module_info: Dict[str, Any]) -> bool:
    """Check if any instantiated sub-modules appear to be sequential (registers)."""
    for inst in module_info.get("instantiations", []):
        module_type = inst["module_type"].lower()
        if "register" in module_type or "reg" in module_type:
            return True
    return False


def create_evaluator(
    module_info: Dict[str, Any],
    filepath: str = "",
    module_name: str = "",
    instance_path: str = "",
    memory_bindings: List[Dict[str, Any]] = None,
    check_submodules: bool = False,
):
    """Create the appropriate evaluator (sequential or combinational) for a parsed module.

    Args:
        module_info: Parsed module dictionary from SystemVerilogParser.
        filepath: Path to the SystemVerilog source file.
        module_name: Override for module name (defaults to module_info["name"]).
        instance_path: Hierarchical instance path for sub-module instantiation.
        memory_bindings: Memory initialization bindings.
        check_submodules: Also check instantiated sub-modules for sequential hints.

    Returns:
        A LogicEvaluator or SequentialLogicEvaluator instance.
    """
    is_sequential = bool(
        module_info.get("sequential_blocks") or module_info.get("clock_signals")
    )
    if not is_sequential and check_submodules:
        is_sequential = _has_sequential_submodules(module_info)

    resolved_name = module_name or module_info.get("name", "")
    source_path = filepath or module_info.get("filepath", "")

    if is_sequential:
        return SequentialLogicEvaluator(
            module_info["inputs"],
            module_info["outputs"],
            module_info["assignments"],
            module_info.get("instantiations", []),
            module_info.get("bus_info", {}),
            module_info.get("slice_assignments", []),
            module_info.get("concat_assignments", []),
            module_info.get("sequential_blocks", []),
            module_info.get("clock_signals", []),
            source_path,
            module_info.get("memory_arrays", {}),
            resolved_name,
            instance_path,
            memory_bindings or [],
            module_info.get("combinational_blocks", []),
        )

    return LogicEvaluator(
        module_info["inputs"],
        module_info["outputs"],
        module_info["assignments"],
        module_info.get("instantiations", []),
        module_info.get("bus_info", {}),
        module_info.get("slice_assignments", []),
        module_info.get("concat_assignments", []),
        source_path,
        module_info.get("memory_arrays", {}),
        resolved_name,
        instance_path,
        memory_bindings or [],
        module_info.get("combinational_blocks", []),
    )


class TruthTableGenerator:
    """Generates and displays truth tables for combinational logic."""

    def __init__(self, evaluator: LogicEvaluator):
        self.evaluator = evaluator

    def generate_truth_table(self, max_combinations: int = 256) -> List[Dict[str, int]]:
        """
        Generate truth table for all input combinations.

        Args:
            max_combinations: Maximum number of input combinations to test

        Returns:
            List of dictionaries containing input and output values for each combination
        """
        inputs = self.evaluator.inputs
        bus_info = self.evaluator.bus_info

        # Calculate total number of input bits
        total_input_bits = 0
        for input_name in inputs:
            if input_name in bus_info:
                total_input_bits += bus_info[input_name]["width"]
            else:
                total_input_bits += 1

        # Limit combinations if too many inputs
        total_combinations = 2**total_input_bits
        if total_combinations > max_combinations:
            print(
                f"Warning: Too many input combinations ({total_combinations}). "
                f"Limiting to first {max_combinations} combinations."
            )

        truth_table = []
        combinations_to_test = min(total_combinations, max_combinations)

        for i in range(combinations_to_test):
            # Convert index to bus values
            input_values = {}
            bit_offset = 0

            for input_name in inputs:
                if input_name in bus_info:
                    width = bus_info[input_name]["width"]
                    # Extract bits for this bus from the combination index
                    bus_value = (i >> (total_input_bits - bit_offset - width)) & (
                        (1 << width) - 1
                    )
                    input_values[input_name] = bus_value
                    bit_offset += width
                else:
                    # Single bit
                    bit_value = (i >> (total_input_bits - bit_offset - 1)) & 1
                    input_values[input_name] = bit_value
                    bit_offset += 1

            # Evaluate outputs
            output_values = self.evaluator.evaluate(input_values)

            # Combine inputs and outputs
            row = {**input_values, **output_values}
            truth_table.append(row)

        return truth_table

    def print_truth_table(self, truth_table: List[Dict[str, int]]):
        """Print a formatted truth table with proper bus formatting."""
        if not truth_table:
            print("No truth table data to display.")
            return

        inputs = self.evaluator.inputs
        outputs = self.evaluator.outputs
        bus_info = self.evaluator.bus_info

        # Create headers with bus information
        input_headers = []
        output_headers = []

        for inp in inputs:
            if inp in bus_info and bus_info[inp]["width"] > 1:
                width = bus_info[inp]["width"]
                msb, lsb = bus_info[inp]["msb"], bus_info[inp]["lsb"]
                input_headers.append(f"{inp}[{msb}:{lsb}]")
            else:
                input_headers.append(inp)

        for out in outputs:
            if out in bus_info and bus_info[out]["width"] > 1:
                width = bus_info[out]["width"]
                msb, lsb = bus_info[out]["msb"], bus_info[out]["lsb"]
                output_headers.append(f"{out}[{msb}:{lsb}]")
            else:
                output_headers.append(out)

        # Print header
        header_inputs = " ".join(f"{header:>6}" for header in input_headers)
        header_outputs = " ".join(f"{header:>6}" for header in output_headers)
        print("Truth Table:")
        print(f"{header_inputs} | {header_outputs}")
        print("-" * (len(header_inputs) + 3 + len(header_outputs)))

        # Print data rows
        for row in truth_table:
            input_values = " ".join(f"{row[inp]:>6}" for inp in inputs)
            output_values = " ".join(f"{row[out]:>6}" for out in outputs)
            print(f"{input_values} | {output_values}")


class TruthTableImageGenerator:
    """Generates truth table images with dark theme and LED indicators."""

    # Color palette
    BG           = (30, 31, 38)       # #1E1F26 charcoal
    ROW_ALT      = (36, 37, 46)       # slightly lighter alt row
    HEADER_BG    = (40, 42, 54)       # header row background
    GRID         = (55, 58, 72)       # subtle grid lines
    DIVIDER      = (80, 85, 105)      # input/output divider
    TEXT_PRIMARY  = (220, 222, 230)   # main text
    TEXT_DIM      = (110, 115, 130)   # dim "0" values
    TEXT_ONE      = (80, 220, 120)    # green "1" values
    ACCENT_INPUT  = (70, 130, 220)   # blue accent for input header
    ACCENT_OUTPUT = (60, 185, 100)   # green accent for output header
    LED_ON        = (60, 220, 110)   # bright green LED
    LED_OFF       = (50, 52, 62)     # dark dim LED
    LED_GLOW      = (60, 220, 110, 40)  # glow overlay (RGBA)
    TITLE_BG      = (24, 25, 32)     # title banner

    # Layout constants
    PAD         = 16   # outer padding
    TITLE_H     = 44   # title banner height
    ACCENT_H    = 3    # colored accent stripe height
    HEADER_H    = 36   # header row height
    ROW_H       = 30   # data row height
    LED_R       = 5    # LED circle radius
    LED_SPACING = 14   # center-to-center between LEDs
    CELL_PAD    = 10   # horizontal padding inside cells

    def __init__(self, evaluator):
        self.evaluator = evaluator
        self.font, self.font_bold, self.font_small = self._load_fonts()

    # Font search paths per platform: (regular, bold)
    FONT_PATHS = [
        ("/System/Library/Fonts/Menlo.ttc",                                     # macOS
         "/System/Library/Fonts/Menlo.ttc"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",                 # Linux (DejaVu)
         "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"),
        ("/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",     # Linux (Liberation)
         "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf"),
        ("C:/Windows/Fonts/consola.ttf",                                        # Windows
         "C:/Windows/Fonts/consolab.ttf"),
    ]

    def _load_fonts(self):
        """Load monospace fonts with platform fallback."""
        regular = [p[0] for p in self.FONT_PATHS]
        bold    = [p[1] for p in self.FONT_PATHS]

        font      = self._try_load_font(regular, 13)
        font_bold = self._try_load_font(bold, 13)
        font_sm   = self._try_load_font(regular, 11)
        return font, font_bold, font_sm

    @staticmethod
    def _try_load_font(candidates, size):
        """Try each font path, falling back to the default bitmap font."""
        for path in candidates:
            try:
                return ImageFont.truetype(path, size)
            except (OSError, IOError):
                continue
        return ImageFont.load_default()

    def generate_image(self, truth_table: List[Dict[str, int]], output_path: str):
        """Generate a PNG image of the truth table."""
        if not truth_table:
            return

        inputs   = self.evaluator.inputs
        outputs  = self.evaluator.outputs
        bus_info = getattr(self.evaluator, 'bus_info', {})

        all_names  = inputs + outputs
        num_inputs = len(inputs)
        widths     = [self._signal_width(name, bus_info) for name in all_names]

        in_headers  = [self._header_label(n, bus_info) for n in inputs]
        out_headers = [self._header_label(n, bus_info) for n in outputs]
        all_headers = in_headers + out_headers

        col_widths = self._calculate_column_widths(all_headers, widths, truth_table, all_names)
        table_w    = sum(col_widths)

        # Image dimensions
        img_w = table_w + 2 * self.PAD
        img_h = (2 * self.PAD + self.TITLE_H + self.ACCENT_H +
                 self.HEADER_H + len(truth_table) * self.ROW_H)

        img  = Image.new("RGBA", (img_w, img_h), self.BG + (255,))
        draw = ImageDraw.Draw(img)

        y = self.PAD
        y = self._draw_title(draw, img_w, y, output_path)
        y = self._draw_header_row(draw, y, col_widths, table_w,
                                  in_headers, out_headers, num_inputs)
        self._draw_data_rows(draw, y, col_widths, table_w,
                             truth_table, all_names, widths, num_inputs)
        self._draw_grid_lines(draw, col_widths, table_w, num_inputs,
                              len(truth_table), self.PAD + self.TITLE_H + self.ACCENT_H)

        if len(truth_table) <= 64:
            img = self._apply_glow(img)

        img.convert("RGB").save(output_path)

    # ── internal helpers ──────────────────────────────────────────

    @staticmethod
    def _signal_width(name, bus_info):
        """Return the bit-width for a signal (1 for single-bit signals)."""
        info = bus_info.get(name)
        if info and info["width"] > 1:
            return info["width"]
        return 1

    @staticmethod
    def _header_label(name, bus_info):
        """Format a signal name as a header label, appending bit range for buses."""
        info = bus_info.get(name)
        if info and info["width"] > 1:
            return f"{name}[{info['msb']}:{info['lsb']}]"
        return name

    def _text_width(self, font, text):
        bbox = font.getbbox(text)
        return bbox[2] - bbox[0]

    def _text_y(self, row_y, font_height):
        """Vertical offset to center text of the given height within a data row."""
        return row_y + (self.ROW_H - font_height) // 2

    def _value_color(self, val):
        """Green for nonzero values, dim for zero."""
        return self.TEXT_ONE if val else self.TEXT_DIM

    def _calculate_column_widths(self, headers, widths, truth_table, all_names):
        """Content-based column widths."""
        col_widths = []
        for i, (header, w) in enumerate(zip(headers, widths)):
            header_px = self._text_width(self.font_bold, header) + 2 * self.CELL_PAD
            if w == 1:
                content_px = (self.LED_R * 2 + 6 +
                              self._text_width(self.font, "0") + 2 * self.CELL_PAD)
            elif w <= 8:
                max_val = max(row[all_names[i]] for row in truth_table) if truth_table else 0
                led_row_px = w * self.LED_SPACING + 4
                content_px = (self._text_width(self.font, str(max_val)) + 8 +
                              led_row_px + 2 * self.CELL_PAD)
            else:
                hex_str = f"0x{(1 << w) - 1:X}"
                content_px = (self._text_width(self.font, hex_str) + 6 +
                              self._text_width(self.font_small, "1" * w) + 2 * self.CELL_PAD)
            col_widths.append(max(header_px, content_px))
        return col_widths

    def _draw_title(self, draw, img_w, y, output_path):
        """Dark banner with module name."""
        draw.rectangle([0, y, img_w, y + self.TITLE_H], fill=self.TITLE_BG)
        title = Path(output_path).stem
        tw = self._text_width(self.font_bold, title)
        draw.text(((img_w - tw) // 2, y + (self.TITLE_H - 16) // 2),
                  title, fill=self.TEXT_PRIMARY, font=self.font_bold)
        return y + self.TITLE_H

    def _draw_header_row(self, draw, y, col_widths, table_w,
                         in_headers, out_headers, num_inputs):
        """Accent stripe + header labels."""
        x0 = self.PAD
        input_w = sum(col_widths[:num_inputs])

        # Input accent stripe (blue), output accent stripe (green)
        draw.rectangle([x0, y, x0 + input_w, y + self.ACCENT_H], fill=self.ACCENT_INPUT)
        draw.rectangle([x0 + input_w, y, x0 + table_w, y + self.ACCENT_H], fill=self.ACCENT_OUTPUT)
        y += self.ACCENT_H

        draw.rectangle([x0, y, x0 + table_w, y + self.HEADER_H], fill=self.HEADER_BG)

        x = x0
        for i, header in enumerate(in_headers + out_headers):
            cw = col_widths[i]
            tw = self._text_width(self.font_bold, header)
            draw.text((x + (cw - tw) // 2, y + (self.HEADER_H - 14) // 2),
                      header, fill=self.TEXT_PRIMARY, font=self.font_bold)
            x += cw

        return y + self.HEADER_H

    def _draw_data_rows(self, draw, y, col_widths, table_w,
                        truth_table, all_names, widths, num_inputs):
        """Render all data rows with appropriate cell types."""
        x0 = self.PAD
        for row_idx, row in enumerate(truth_table):
            bg = self.ROW_ALT if row_idx % 2 else self.BG
            ry = y + row_idx * self.ROW_H
            draw.rectangle([x0, ry, x0 + table_w, ry + self.ROW_H], fill=bg)

            x = x0
            for col_idx, name in enumerate(all_names):
                cw = col_widths[col_idx]
                val = row[name]
                w = widths[col_idx]
                if w == 1:
                    self._draw_bit_cell(draw, x, ry, cw, val)
                elif w <= 8:
                    self._draw_bus_cell(draw, x, ry, cw, val, w)
                else:
                    self._draw_large_bus_cell(draw, x, ry, val, w)
                x += cw

    def _draw_bit_cell(self, draw, x, y, cw, val):
        """Single-bit: '0'/'1' text (left) + LED circle (right)."""
        cy = y + self.ROW_H // 2
        tx = x + self.CELL_PAD
        draw.text((tx, self._text_y(y, 13)), str(val),
                  fill=self._value_color(val), font=self.font)
        led_x = x + cw - self.CELL_PAD - self.LED_R
        self._draw_led(draw, led_x, cy, val)

    def _draw_bus_cell(self, draw, x, y, cw, val, width):
        """Bus 2-8 bits: decimal value (left) + LED row right-aligned (MSB to LSB)."""
        cy = y + self.ROW_H // 2
        val_str = str(val)
        tx = x + self.CELL_PAD
        draw.text((tx, self._text_y(y, 13)), val_str,
                  fill=self._value_color(val), font=self.font)

        # Right-align LEDs: last LED's right edge at cell right padding
        last_led_cx = x + cw - self.CELL_PAD - self.LED_R
        first_led_cx = last_led_cx - (width - 1) * self.LED_SPACING
        lx = first_led_cx
        for bit in range(width - 1, -1, -1):
            self._draw_led(draw, lx, cy, (val >> bit) & 1)
            lx += self.LED_SPACING

    def _draw_large_bus_cell(self, draw, x, y, val, width):
        """Bus 9+ bits: hex + binary string."""
        hex_digits = (width + 3) // 4
        hex_str = f"0x{val:0{hex_digits}X}"
        bin_str = format(val, f'0{width}b')
        tx = x + self.CELL_PAD
        draw.text((tx, self._text_y(y, 13)), hex_str,
                  fill=self._value_color(val), font=self.font)
        draw.text((tx + self._text_width(self.font, hex_str) + 6, self._text_y(y, 12)),
                  bin_str, fill=self.TEXT_DIM, font=self.font_small)

    def _draw_led(self, draw, cx, cy, on):
        """Draw a single LED circle."""
        r = self.LED_R
        color = self.LED_ON if on else self.LED_OFF
        draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)

    def _draw_grid_lines(self, draw, col_widths, table_w, num_inputs, num_rows, header_top):
        """Subtle grid lines and input/output divider."""
        x0 = self.PAD
        table_bottom = header_top + self.HEADER_H + num_rows * self.ROW_H

        for i in range(num_rows + 1):
            ly = header_top + self.HEADER_H + i * self.ROW_H
            draw.line([x0, ly, x0 + table_w, ly], fill=self.GRID, width=1)

        x = x0
        for i, cw in enumerate(col_widths):
            x += cw
            if x < x0 + table_w:
                is_divider = (i == num_inputs - 1)
                lw = 2 if is_divider else 1
                color = self.DIVIDER if is_divider else self.GRID
                draw.line([x, header_top, x, table_bottom], fill=color, width=lw)

    def _apply_glow(self, img):
        """Add subtle glow around lit LEDs."""
        glow_layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
        pixels = img.load()
        glow_pixels = glow_layer.load()
        for py in range(img.height):
            for px in range(img.width):
                r, g, b, a = pixels[px, py]
                if g > 180 and r < 100 and b < 150:
                    glow_pixels[px, py] = self.LED_GLOW
        glow_layer = glow_layer.filter(ImageFilter.GaussianBlur(radius=4))
        return Image.alpha_composite(img, glow_layer)


class WaveformImageGenerator:
    """Generates waveform images with dark theme matching truth table style."""

    # Color palette (shared with TruthTableImageGenerator)
    BG          = (30, 31, 38)
    TITLE_BG    = (24, 25, 32)
    HEADER_BG   = (40, 42, 54)
    GRID        = (55, 58, 72)
    TEXT_PRIMARY = (220, 222, 230)
    TEXT_DIM     = (110, 115, 130)

    # Signal colors
    CLK_COLOR    = (230, 75, 60)     # warm red
    INPUT_COLOR  = (70, 150, 230)    # blue (matches ACCENT_INPUT)
    OUTPUT_COLOR = (60, 220, 110)    # green (matches LED_ON)

    # Layout
    PAD       = 16
    TITLE_H   = 44
    SIGNAL_H  = 40
    CYCLE_W   = 80
    CYCLE_LBL = 24    # cycle number label height
    ACCENT_W  = 3     # color stripe next to labels
    WAVE_PAD  = 8     # vertical padding within signal row

    FONT_PATHS = [
        ("/System/Library/Fonts/Menlo.ttc",
         "/System/Library/Fonts/Menlo.ttc"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
         "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"),
        ("/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
         "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf"),
        ("C:/Windows/Fonts/consola.ttf",
         "C:/Windows/Fonts/consolab.ttf"),
    ]

    def __init__(self, evaluator):
        self.evaluator = evaluator
        self.font, self.font_bold, self.font_small = self._load_fonts()

    def _load_fonts(self):
        regular = [p[0] for p in self.FONT_PATHS]
        bold    = [p[1] for p in self.FONT_PATHS]
        return (self._try_load_font(regular, 13),
                self._try_load_font(bold, 13),
                self._try_load_font(regular, 11))

    @staticmethod
    def _try_load_font(candidates, size):
        for path in candidates:
            try:
                return ImageFont.truetype(path, size)
            except (OSError, IOError):
                continue
        return ImageFont.load_default()

    # ── public entry point ───────────────────────────────────────

    def generate_image(self, test_results: List[Dict], output_path: str):
        if not test_results:
            return

        clocks, inputs, outputs = self._classify_signals(test_results)
        signals = clocks + inputs + outputs
        num_signals = len(signals)
        num_cycles = len(test_results)

        # Dynamic label column width
        label_w = self._calc_label_width(signals, clocks, inputs)

        grid_w = num_cycles * self.CYCLE_W
        img_w = self.PAD + label_w + grid_w + self.PAD
        img_h = (self.PAD + self.TITLE_H + num_signals * self.SIGNAL_H
                 + self.CYCLE_LBL + self.PAD)

        img = Image.new("RGB", (img_w, img_h), self.BG)
        draw = ImageDraw.Draw(img)

        x0 = self.PAD + label_w          # grid left edge
        y0 = self.PAD + self.TITLE_H     # grid top edge

        self._draw_title(draw, img_w, self.PAD, output_path)
        self._draw_grid(draw, x0, y0, num_cycles, num_signals)
        self._draw_signal_labels(draw, signals, clocks, inputs, y0, label_w)
        self._draw_cycle_labels(draw, x0,
                                y0 + num_signals * self.SIGNAL_H, num_cycles)

        for i, signal in enumerate(signals):
            values = self._extract_values(test_results, signal)
            color = self._signal_color(signal, clocks, inputs)
            y = y0 + i * self.SIGNAL_H

            if signal in clocks:
                self._draw_clock_waveform(draw, values, x0, y, color)
            elif max(values) > 1:
                self._draw_multibit_waveform(draw, values, x0, y, color)
            else:
                self._draw_digital_waveform(draw, values, x0, y, color)

        img.save(output_path)

    # ── signal helpers ───────────────────────────────────────────

    @staticmethod
    def _classify_signals(test_results):
        clock_s, input_s, output_s = set(), set(), set()
        for r in test_results:
            for sig in r.get('inputs', {}):
                if 'clk' in sig.lower() or 'clock' in sig.lower():
                    clock_s.add(sig)
                else:
                    input_s.add(sig)
            output_s.update(r.get('outputs', {}))
        return sorted(clock_s), sorted(input_s), sorted(output_s)

    def _signal_color(self, signal, clocks, inputs):
        if signal in clocks:
            return self.CLK_COLOR
        return self.INPUT_COLOR if signal in inputs else self.OUTPUT_COLOR

    @staticmethod
    def _extract_values(test_results, signal):
        values = []
        for r in test_results:
            if signal in r.get('inputs', {}):
                values.append(r['inputs'][signal])
            elif signal in r.get('outputs', {}):
                values.append(r['outputs'][signal])
            else:
                values.append(0)
        return values

    def _signal_label(self, signal, clocks, inputs):
        if signal in clocks:
            return f"{signal} (clk)"
        return f"{signal} (in)" if signal in inputs else f"{signal} (out)"

    # ── layout helpers ───────────────────────────────────────────

    def _text_width(self, font, text):
        bbox = font.getbbox(text)
        return bbox[2] - bbox[0]

    def _calc_label_width(self, signals, clocks, inputs):
        max_tw = 0
        for sig in signals:
            tw = self._text_width(self.font_bold,
                                  self._signal_label(sig, clocks, inputs))
            max_tw = max(max_tw, tw)
        return max(140, max_tw + self.ACCENT_W + 2 * self.PAD + 8)

    # ── drawing routines ─────────────────────────────────────────

    def _draw_title(self, draw, img_w, y, output_path):
        draw.rectangle([0, y, img_w, y + self.TITLE_H], fill=self.TITLE_BG)
        title = Path(output_path).stem
        tw = self._text_width(self.font_bold, title)
        draw.text(((img_w - tw) // 2, y + (self.TITLE_H - 16) // 2),
                  title, fill=self.TEXT_PRIMARY, font=self.font_bold)

    def _draw_signal_labels(self, draw, signals, clocks, inputs, y0, label_w):
        text_right = self.PAD + label_w - self.ACCENT_W - 8
        for i, signal in enumerate(signals):
            color = self._signal_color(signal, clocks, inputs)
            y = y0 + i * self.SIGNAL_H
            # Accent stripe
            sx = self.PAD + label_w - self.ACCENT_W
            draw.rectangle([sx, y, sx + self.ACCENT_W, y + self.SIGNAL_H],
                           fill=color)
            # Right-aligned label
            label = self._signal_label(signal, clocks, inputs)
            tw = self._text_width(self.font_bold, label)
            draw.text((text_right - tw, y + (self.SIGNAL_H - 14) // 2),
                      label, fill=self.TEXT_DIM, font=self.font_bold)

    def _draw_grid(self, draw, x0, y0, num_cycles, num_signals):
        gw = num_cycles * self.CYCLE_W
        gh = num_signals * self.SIGNAL_H
        for c in range(num_cycles + 1):
            x = x0 + c * self.CYCLE_W
            draw.line([x, y0, x, y0 + gh], fill=self.GRID, width=1)
        for s in range(num_signals + 1):
            y = y0 + s * self.SIGNAL_H
            draw.line([x0, y, x0 + gw, y], fill=self.GRID, width=1)

    def _draw_cycle_labels(self, draw, x0, y, num_cycles):
        for c in range(num_cycles):
            label = str(c)
            tw = self._text_width(self.font_small, label)
            cx = x0 + c * self.CYCLE_W + (self.CYCLE_W - tw) // 2
            draw.text((cx, y + 4), label, fill=self.TEXT_DIM, font=self.font_small)

    # ── waveform renderers ───────────────────────────────────────

    def _draw_clock_waveform(self, draw, values, x0, y, color):
        y_lo = y + self.SIGNAL_H - self.WAVE_PAD
        y_hi = y + self.WAVE_PAD
        lw = 2
        for c in range(len(values)):
            xs = x0 + c * self.CYCLE_W
            xm = xs + self.CYCLE_W // 2
            xe = xs + self.CYCLE_W
            draw.line([xs, y_lo, xs, y_hi], fill=color, width=lw)
            draw.line([xs, y_hi, xm, y_hi], fill=color, width=lw)
            draw.line([xm, y_hi, xm, y_lo], fill=color, width=lw)
            draw.line([xm, y_lo, xe, y_lo], fill=color, width=lw)

    def _draw_digital_waveform(self, draw, values, x0, y, color):
        y_lo = y + self.SIGNAL_H - self.WAVE_PAD
        y_hi = y + self.WAVE_PAD
        lw = 2
        prev = y_hi if values[0] else y_lo
        for c in range(len(values)):
            xs = x0 + c * self.CYCLE_W
            xe = xs + self.CYCLE_W
            cur = y_hi if values[c] else y_lo
            if c > 0 and prev != cur:
                draw.line([xs, prev, xs, cur], fill=color, width=lw)
            draw.line([xs, cur, xe, cur], fill=color, width=lw)
            prev = cur

    def _draw_multibit_waveform(self, draw, values, x0, y, color):
        y_top = y + self.WAVE_PAD
        y_bot = y + self.SIGNAL_H - self.WAVE_PAD
        y_mid = (y_top + y_bot) // 2
        lw = 2
        xw = 6                    # X-crossing half-width
        n = len(values)
        bg = tuple(max(0, ch // 3) for ch in color)

        for c in range(n):
            xl = x0 + c * self.CYCLE_W
            xr = xl + self.CYCLE_W
            changed_l = c > 0 and values[c] != values[c - 1]
            changed_r = c < n - 1 and values[c] != values[c + 1]

            # Horizontal bus lines (shortened around X crossings)
            hx0 = xl + xw if changed_l else xl
            hx1 = xr - xw if changed_r else xr
            draw.line([hx0, y_top, hx1, y_top], fill=color, width=lw)
            draw.line([hx0, y_bot, hx1, y_bot], fill=color, width=lw)

            # X crossing at left boundary
            if changed_l:
                draw.line([xl - xw, y_top, xl + xw, y_bot], fill=color, width=lw)
                draw.line([xl - xw, y_bot, xl + xw, y_top], fill=color, width=lw)

            # Value label centred in available space
            val_s = str(values[c])
            tw = self._text_width(self.font_small, val_s)
            tx = hx0 + (hx1 - hx0 - tw) // 2
            draw.rounded_rectangle([tx - 4, y_mid - 8, tx + tw + 4, y_mid + 8],
                                   radius=4, fill=bg)
            draw.text((tx, y_mid - 6), val_s, fill=color, font=self.font_small)


class TestRunner:
    """Runs test cases from JSON files against the simulator."""

    def __init__(self, evaluator: Any, verbose: bool = True):
        self.evaluator = evaluator
        self.verbose = verbose
        self.test_cycles = []  # Store test cycles for waveform generation
        self.test_outputs = []  # Store per-test pass/fail output for callers
        self.loaded_test_file = ""

    def _emit(self, message: str, record: bool = True):
        """Emit output while optionally recording it for silent callers."""
        if record and message:
            self.test_outputs.append(message)
        if self.verbose:
            print(message)

    def _evaluate_inputs(self, input_values: Dict[str, Any]) -> Dict[str, int]:
        """Evaluate one input set for combinational or sequential designs."""
        if hasattr(self.evaluator, "evaluate_cycle"):
            return self.evaluator.evaluate_cycle(input_values)
        return self.evaluator.evaluate(input_values)

    def _check_expected_outputs(
        self,
        label: str,
        actual_outputs: Dict[str, int],
        expected_outputs: Dict[str, Any],
        description: str = "",
        emit_pass: bool = True,
    ) -> bool:
        """Compare actual outputs to expectations and emit consistent messages."""
        suffix = f" - {description}" if description else ""
        test_passed = True
        for output_name, expected_value in expected_outputs.items():
            if output_name not in actual_outputs:
                self._emit(
                    f"{label} failed: Output '{output_name}' not found{suffix}"
                )
                test_passed = False
            elif actual_outputs[output_name] != expected_value:
                self._emit(
                    f"{label} failed: {output_name} = {actual_outputs[output_name]}, "
                    f"expected {expected_value}{suffix}"
                )
                test_passed = False

        if test_passed and emit_pass:
            self._emit(f"{label} passed{suffix}")
        return test_passed

    def load_tests(self, test_file: str) -> Any:
        """Load test cases from a JSON file."""
        try:
            with open(test_file, "r", encoding="utf-8") as f:
                tests = json.load(f)
            self.loaded_test_file = test_file
            return tests
        except FileNotFoundError:
            raise FileNotFoundError(f"Test file not found: {test_file}")
        except json.JSONDecodeError as e:
            raise ValueError(f"Invalid JSON in test file {test_file}: {e}")

    def run_tests(self, tests) -> Tuple[int, int]:
        """
        Run all test cases and return pass/fail counts.
        Supports both combinational and sequential test formats.

        Args:
            tests: List of test case dictionaries or sequential test format dict

        Returns:
            Tuple of (passed_count, total_count)
        """
        self.test_outputs = []

        # Configure ROM/RAM bindings before running cycles.
        test_dir = os.path.dirname(self.loaded_test_file) if self.loaded_test_file else os.getcwd()
        default_module = getattr(self.evaluator, "module_name", "")
        memory_bindings = normalize_memory_bindings(tests, test_dir, default_module)
        if hasattr(self.evaluator, "configure_memory_bindings"):
            self.evaluator.configure_memory_bindings(memory_bindings)

        # Check if this is the new sequential test format
        if isinstance(tests, dict) and (tests.get('sequential') or tests.get('test_cases')):
            return self._run_new_sequential_tests(tests)
        # Check if this is the old sequential test format
        elif isinstance(tests, dict) and tests.get('test_type') == 'sequential':
            return self._run_sequential_tests(tests)
        else:
            return self._run_combinational_tests(tests)
    
    def _run_combinational_tests(self, tests: List[Dict[str, Any]]) -> Tuple[int, int]:
        """Run combinational logic tests (original format)"""
        if self.verbose:
            print("\nRunning combinational tests...")
        passed = 0
        total = len(tests)

        for i, test in enumerate(tests, 1):
            # Extract input values (all keys except 'expect')
            input_values = {k: v for k, v in test.items() if k != "expect"}
            expected_outputs = test.get("expect", {})

            # Run simulation
            actual_outputs = self.evaluator.evaluate(input_values)

            if self._check_expected_outputs(
                f"Test {i}", actual_outputs, expected_outputs
            ):
                passed += 1

        return passed, total
    
    def _run_sequential_tests(self, test_data: Dict[str, Any]) -> Tuple[int, int]:
        """Run sequential logic tests (legacy cycle-based format)."""
        if self.verbose:
            print("\nRunning sequential tests...")
        test_cycles = test_data.get('test_cycles', [])
        passed = 0
        total = len(test_cycles)
        
        # Reset sequential state if available
        if hasattr(self.evaluator, 'reset_state'):
            self.evaluator.reset_state()
        
        # Clear previous test cycles
        self.test_cycles = []
        
        for i, cycle_test in enumerate(test_cycles):
            cycle_num = cycle_test.get('cycle', i)
            input_values = cycle_test.get('inputs', {})
            expected_outputs = cycle_test.get('expected_outputs', {})
            description = cycle_test.get('description', f'Cycle {cycle_num}')
            
            # Run one clock cycle
            actual_outputs = self._evaluate_inputs(input_values)
            
            # Store cycle data for waveform generation
            self.test_cycles.append({
                'cycle': cycle_num,
                'inputs': input_values.copy(),
                'outputs': actual_outputs.copy(),
                'description': description
            })
            
            if self._check_expected_outputs(
                f"Cycle {cycle_num}",
                actual_outputs,
                expected_outputs,
                description,
            ):
                passed += 1
        
        return passed, total
    
    def _run_new_sequential_tests(self, test_data: Dict[str, Any]) -> Tuple[int, int]:
        """Run new sequential logic tests format"""
        if self.verbose:
            print("\nRunning sequential tests...")
        test_cases = test_data.get('test_cases', [])
        passed = 0
        total = 0
        
        # Reset sequential state if available
        if hasattr(self.evaluator, 'reset_state'):
            self.evaluator.reset_state()
        
        # Clear previous test cycles
        self.test_cycles = []
        cycle_counter = 0
        
        for test_case in test_cases:
            name = test_case.get('name', 'Unnamed test')
            
            if 'sequence' in test_case:
                # Handle sequence tests
                sequence_passed = True
                for step in test_case['sequence']:
                    input_values = step.get('inputs', {})
                    expected_outputs = step.get('expected', {})
                    
                    # Run one clock cycle
                    actual_outputs = self._evaluate_inputs(input_values)
                    
                    # Store cycle data for waveform generation
                    self.test_cycles.append({
                        'cycle': cycle_counter,
                        'inputs': input_values.copy(),
                        'outputs': actual_outputs.copy(),
                        'description': f'{name} - Step {cycle_counter}'
                    })
                    cycle_counter += 1
                    
                    if not self._check_expected_outputs(
                        name,
                        actual_outputs,
                        expected_outputs,
                        emit_pass=False,
                    ):
                        sequence_passed = False
                
                if sequence_passed:
                    self._emit(f"{name} passed")
                    passed += 1
                total += 1
            
            else:
                # Handle single test cases
                input_values = test_case.get('inputs', {})
                expected_outputs = test_case.get('expected', {})
                
                # Run one clock cycle
                actual_outputs = self._evaluate_inputs(input_values)
                
                # Store cycle data for waveform generation
                self.test_cycles.append({
                    'cycle': cycle_counter,
                    'inputs': input_values.copy(),
                    'outputs': actual_outputs.copy(),
                    'description': name
                })
                cycle_counter += 1
                
                if self._check_expected_outputs(
                    name, actual_outputs, expected_outputs
                ):
                    passed += 1
                total += 1
        
        return passed, total


def _check_missing_expect_fields(tests) -> str:
    """Check if any test cases are missing expect/expected fields."""
    missing_count = 0

    if isinstance(tests, dict):
        if tests.get("test_type") == "sequential":
            for cycle in tests.get("test_cycles", []):
                if not cycle.get("expected_outputs"):
                    missing_count += 1
        elif tests.get("sequential") or tests.get("test_cases"):
            for test_case in tests.get("test_cases", []):
                if "sequence" in test_case:
                    for step in test_case["sequence"]:
                        if not step.get("expected"):
                            missing_count += 1
                elif not test_case.get("expected"):
                    missing_count += 1
    elif isinstance(tests, list):
        for test in tests:
            if not test.get("expect"):
                missing_count += 1

    if missing_count > 0:
        return f"Warning: {missing_count} test(s) missing expect field"
    return ""


def _append_warning(existing: str, new_warning: str) -> str:
    """Append a warning using the existing semicolon-delimited format."""
    if not new_warning:
        return existing
    if existing:
        return f"{existing}; {new_warning}"
    return new_warning


def _find_json_test_file(sv_file: str) -> Optional[str]:
    """Find the corresponding JSON test file for a SystemVerilog file."""
    sv_path = Path(sv_file)
    possible_names = [
        sv_path.with_suffix(".json"),
        sv_path.parent / f"{sv_path.stem}_test.json",
        sv_path.parent / f"{sv_path.stem}_tests.json",
    ]
    for json_path in possible_names:
        if json_path.exists():
            return str(json_path)
    return None


def _generate_truth_table(
    evaluator: Any, max_combinations: int
) -> Tuple[List[Dict[str, int]], bool, str]:
    """Generate a truth table and capture any warning output."""
    truth_table: List[Dict[str, int]] = []
    truth_table_success = True
    warnings = ""

    if hasattr(evaluator, "evaluate_cycle"):
        return truth_table, True, "Truth table skipped for sequential logic module"

    try:
        capture = io.StringIO()
        with contextlib.redirect_stdout(capture):
            truth_table_gen = TruthTableGenerator(evaluator)
            truth_table = truth_table_gen.generate_truth_table(max_combinations)
        warnings = capture.getvalue().strip()
    except Exception as e:
        truth_table_success = False
        warnings = f"Truth table generation failed: {e}"

    return truth_table, truth_table_success, warnings


def _run_json_tests(evaluator: Any, json_file: str) -> Dict[str, Any]:
    """Run JSON-backed tests using the shared simulator-side test runner."""
    sim_runner = TestRunner(evaluator, verbose=False)
    tests = sim_runner.load_tests(json_file)
    missing_expect_warning = _check_missing_expect_fields(tests)
    passed_tests, total_tests = sim_runner.run_tests(tests)
    return {
        "passed_tests": passed_tests,
        "total_tests": total_tests,
        "test_success": passed_tests == total_tests,
        "test_outputs": sim_runner.test_outputs,
        "test_cycles": sim_runner.test_cycles,
        "warning": missing_expect_warning,
    }


def _generate_output_image(
    evaluator: Any,
    sv_file: str,
    truth_table: List[Dict[str, int]],
    test_cycles: List[Dict[str, Any]],
) -> Tuple[Optional[str], str]:
    """Generate the PNG artifact associated with a file's results."""
    try:
        png_path = str(Path(sv_file).with_suffix(".png"))
        if hasattr(evaluator, "evaluate_cycle"):
            if test_cycles:
                waveform_gen = WaveformImageGenerator(evaluator)
                waveform_gen.generate_image(test_cycles, png_path)
                return png_path, ""
            return None, ""

        if truth_table:
            image_gen = TruthTableImageGenerator(evaluator)
            image_gen.generate_image(truth_table, png_path)
            return png_path, ""
    except Exception as e:
        return None, f"Image generation failed: {e}"

    return None, ""


def _analyze_sv_file(sv_file: str, max_combinations: int = 16) -> Dict[str, Any]:
    """Process one SystemVerilog file into a serializable result payload."""
    try:
        start_time = time.time()
        clear_module_cache()

        json_file = _find_json_test_file(sv_file)
        parser = SystemVerilogParser()
        module_info = parser.parse_file(sv_file)
        evaluator = create_evaluator(module_info, filepath=sv_file, check_submodules=True)
        nand_gate_count = evaluator.count_nand_gates()

        truth_table, truth_table_success, warnings = _generate_truth_table(
            evaluator, max_combinations
        )
        test_cycles: List[Dict[str, Any]] = []
        passed_tests = 0
        total_tests = 0
        test_success = True
        test_outputs: List[str] = []
        error_message = ""

        if json_file:
            try:
                test_result = _run_json_tests(evaluator, json_file)
                passed_tests = test_result["passed_tests"]
                total_tests = test_result["total_tests"]
                test_success = test_result["test_success"]
                test_outputs = test_result["test_outputs"]
                test_cycles = test_result["test_cycles"]
                warnings = _append_warning(warnings, test_result["warning"])
            except Exception as e:
                test_success = False
                error_message = f"Test execution failed: {e}"
                test_outputs = []

        png_file, image_warning = _generate_output_image(
            evaluator, sv_file, truth_table, test_cycles
        )
        warnings = _append_warning(warnings, image_warning)
        execution_time = time.time() - start_time

        return {
            "sv_file": sv_file,
            "json_file": json_file,
            "success": truth_table_success and (not json_file or test_success),
            "parse_success": True,
            "truth_table_success": truth_table_success,
            "test_success": test_success,
            "passed_tests": passed_tests,
            "total_tests": total_tests,
            "error_message": error_message,
            "truth_table": truth_table,
            "execution_time": execution_time,
            "nand_gate_count": nand_gate_count,
            "warnings": warnings,
            "test_outputs": test_outputs,
            "inputs": module_info["inputs"],
            "outputs": module_info["outputs"],
            "bus_info": module_info.get("bus_info", {}),
            "png_file": png_file,
            "module_name": module_info.get("name", Path(sv_file).stem),
            "is_sequential": hasattr(evaluator, "evaluate_cycle"),
        }
    except Exception as e:
        return {
            "sv_file": sv_file,
            "json_file": None,
            "success": False,
            "parse_success": False,
            "truth_table_success": False,
            "test_success": False,
            "passed_tests": 0,
            "total_tests": 0,
            "error_message": f"Processing failed: {e}",
            "truth_table": [],
            "execution_time": 0.0,
            "nand_gate_count": 0,
            "warnings": "",
            "test_outputs": [],
            "inputs": [],
            "outputs": [],
            "bus_info": {},
            "png_file": None,
            "module_name": Path(sv_file).stem,
            "is_sequential": False,
        }


def test_single_file_standalone(sv_file: str, max_combinations: int = 16):
    """Standalone helper used by process workers."""
    return _analyze_sv_file(sv_file, max_combinations)


class TestReport:
    """Container for test results and statistics."""

    def __init__(self, sv_file: str):
        self.sv_file = sv_file
        self.json_file = None
        self.success = False
        self.parse_success = False
        self.truth_table_success = False
        self.test_success = False
        self.passed_tests = 0
        self.total_tests = 0
        self.error_message = ""
        self.truth_table = []
        self.evaluator = None
        self.execution_time = 0.0
        self.nand_gate_count = 0
        self.warnings = ""
        self.test_outputs = []
        self.png_file = None
        self.module_name = Path(sv_file).stem
        self.is_sequential = False

    @property
    def has_tests(self) -> bool:
        return self.json_file is not None

    @property
    def test_pass_rate(self) -> float:
        if self.total_tests == 0:
            return 0.0
        return (self.passed_tests / self.total_tests) * 100


class SystemVerilogTestRunner:
    """Run tests for one file or a directory tree of SystemVerilog modules."""

    def __init__(self, parallel: bool = True, max_workers: Optional[int] = None):
        self.max_combinations = 16
        self.continue_on_error = True
        self.reports: List[TestReport] = []
        self.parallel = parallel
        if max_workers is None:
            self.max_workers = max(1, multiprocessing.cpu_count() - 1)
        else:
            self.max_workers = max_workers
        self.run_failed = False
        self.run_failure_message = ""

    def find_sv_files(self, path: str) -> List[str]:
        """Find all SystemVerilog files in the given path."""
        path_obj = Path(path)

        if path_obj.is_file() and path_obj.suffix == ".sv":
            return [str(path_obj)]
        if path_obj.is_dir():
            return [str(file_path) for file_path in sorted(path_obj.rglob("*.sv"))]
        raise ValueError(f"Path '{path}' is not a valid file or directory")

    def find_json_test(self, sv_file: str) -> Optional[str]:
        """Find the corresponding JSON test file for a SystemVerilog file."""
        return _find_json_test_file(sv_file)

    def _report_from_result(self, result_dict: Dict[str, Any]) -> TestReport:
        """Convert a serializable result payload into a TestReport."""
        report = TestReport(result_dict["sv_file"])
        report.json_file = result_dict["json_file"]
        report.success = result_dict["success"]
        report.parse_success = result_dict["parse_success"]
        report.truth_table_success = result_dict["truth_table_success"]
        report.test_success = result_dict["test_success"]
        report.passed_tests = result_dict["passed_tests"]
        report.total_tests = result_dict["total_tests"]
        report.error_message = result_dict["error_message"]
        report.truth_table = result_dict["truth_table"]
        report.execution_time = result_dict["execution_time"]
        report.nand_gate_count = result_dict["nand_gate_count"]
        report.warnings = result_dict["warnings"]
        report.test_outputs = result_dict["test_outputs"]
        report.png_file = result_dict.get("png_file")
        report.module_name = result_dict.get("module_name", report.module_name)
        report.is_sequential = result_dict.get("is_sequential", False)

        class DummyEvaluator:
            def __init__(self, inputs, outputs, bus_info, module_name):
                self.inputs = inputs
                self.outputs = outputs
                self.bus_info = bus_info or {}
                self.module_name = module_name

        report.evaluator = DummyEvaluator(
            result_dict.get("inputs", []),
            result_dict.get("outputs", []),
            result_dict.get("bus_info", {}),
            report.module_name,
        )
        return report

    def test_single_file(self, sv_file: str) -> TestReport:
        """Test a single SystemVerilog file."""
        return self._report_from_result(_analyze_sv_file(sv_file, self.max_combinations))

    def run_tests(self, path: str) -> None:
        """Run tests for all SystemVerilog files in the given path."""
        self.run_failed = False
        self.run_failure_message = ""
        self.reports = []
        try:
            sv_files = self.find_sv_files(path)
            if not sv_files:
                print(f"No SystemVerilog files found in: {path}")
                return

            print(f"Found {len(sv_files)} SystemVerilog file(s) to test\n")

            should_run_parallel = self.parallel and self.max_workers > 1 and len(sv_files) > 1
            if should_run_parallel:
                try:
                    self._run_tests_parallel(sv_files)
                except Exception as e:
                    print(f"[WARN] Parallel execution unavailable: {e}")
                    print("[INFO] Falling back to sequential execution\n")
                    self._run_tests_sequential(sv_files)
            else:
                self._run_tests_sequential(sv_files)

        except KeyboardInterrupt:
            self.run_failed = True
            self.run_failure_message = "Test run interrupted by user"
            print("\n[INFO] Test run interrupted by user")
        except Exception as e:
            self.run_failed = True
            self.run_failure_message = str(e)
            print(f"[ERROR] Test runner failed: {e}")
            traceback.print_exc()

    def _run_tests_sequential(self, sv_files: List[str]) -> None:
        """Run tests sequentially."""
        for sv_file in sv_files:
            report = self.test_single_file(sv_file)
            self.reports.append(report)
            self.print_file_report(report)

            if not self.continue_on_error and not report.success:
                print(f"Stopping due to error in: {sv_file}")
                break

    def _run_tests_parallel(self, sv_files: List[str]) -> None:
        """Run tests in parallel using ProcessPoolExecutor."""
        print(f"Running tests in parallel with {self.max_workers} workers...\n")

        with ProcessPoolExecutor(max_workers=self.max_workers) as executor:
            future_to_index = {}
            for index, sv_file in enumerate(sv_files):
                future = executor.submit(
                    test_single_file_standalone, sv_file, self.max_combinations
                )
                future_to_index[future] = index

            reports_by_index = {}
            for future in as_completed(future_to_index):
                index = future_to_index[future]
                sv_file = sv_files[index]
                try:
                    reports_by_index[index] = self._report_from_result(future.result())
                except Exception as e:
                    error_report = TestReport(sv_file)
                    error_report.error_message = f"Parallel execution failed: {e}"
                    reports_by_index[index] = error_report

            ordered_reports = []
            for index in range(len(sv_files)):
                report = reports_by_index[index]
                ordered_reports.append(report)
                self.print_file_report(report)

            self.reports.extend(ordered_reports)

    def print_file_report(self, report: TestReport) -> None:
        """Print a detailed report for a single file."""
        print("=" * 80)
        print(f"FILE: {report.sv_file}")
        print("=" * 80)

        status = "PASS" if report.success else "FAIL"
        print(f"Status: [{status}]")
        print(f"Module: {report.module_name}")
        print(f"Inputs: {report.evaluator.inputs if report.evaluator else 'N/A'}")
        print(f"Outputs: {report.evaluator.outputs if report.evaluator else 'N/A'}")
        print(f"NAND Gates: {report.nand_gate_count}")
        print(f"Execution Time: {report.execution_time:.3f}s")
        if report.png_file:
            print(f"PNG Output: {report.png_file}")

        if report.has_tests:
            print(f"JSON Test File: {report.json_file}")
            print(
                f"Test Results: {report.passed_tests}/{report.total_tests} passed "
                f"({report.test_pass_rate:.1f}%)"
            )
        else:
            print("JSON Test File: None")
            print("Test Results: No tests")

        if report.warnings:
            print(f"Warnings: {report.warnings}")

        if report.test_outputs:
            print("\nTest Execution:")
            for output in report.test_outputs:
                print(f"  {output}")

        if report.error_message:
            print(f"Error: {report.error_message}")

        if report.is_sequential:
            print("\nTruth Table: Skipped (sequential logic module)")
        elif report.truth_table and report.truth_table_success and report.evaluator:
            print()
            try:
                truth_table_gen = TruthTableGenerator(report.evaluator)
                truth_table_gen.print_truth_table(report.truth_table)
            except Exception as e:
                print(f"Truth Table Error: {e}")
        elif report.truth_table_success:
            print(f"\nTruth Table: {len(report.truth_table)} combinations generated")
        else:
            print("\nTruth Table: Failed to generate")

        print()

    def print_summary_report(self) -> None:
        """Print summary statistics for the test run."""
        total_files = len(self.reports)
        if total_files == 0:
            print("\nNo files tested.")
            if self.run_failed and self.run_failure_message:
                print(f"Run failure: {self.run_failure_message}")
            return

        successful_files = sum(1 for report in self.reports if report.success)
        parse_failures = sum(1 for report in self.reports if not report.parse_success)
        truth_table_failures = sum(
            1 for report in self.reports if not report.truth_table_success
        )

        files_with_tests = sum(1 for report in self.reports if report.has_tests)
        test_failures = sum(
            1 for report in self.reports if report.has_tests and not report.test_success
        )
        total_test_cases = sum(report.total_tests for report in self.reports)
        passed_test_cases = sum(report.passed_tests for report in self.reports)

        total_time = sum(report.execution_time for report in self.reports)
        total_nand_gates = sum(report.nand_gate_count for report in self.reports)
        avg_nand_gates = total_nand_gates / total_files if total_files > 0 else 0

        print("\n" + "=" * 60)
        print("SUMMARY REPORT")
        print("=" * 60)
        print(f"Files Tested:           {total_files}")
        print(
            f"Overall Success:        {successful_files}/{total_files} "
            f"({successful_files / total_files * 100:.1f}%)"
        )
        print(f"Parse Failures:         {parse_failures}")
        print(f"Truth Table Failures:   {truth_table_failures}")
        print(f"Files with JSON Tests:  {files_with_tests}")
        print(f"Test Case Failures:     {test_failures}")
        print(f"Total Test Cases:       {total_test_cases}")
        print(
            f"Passed Test Cases:      {passed_test_cases}/{total_test_cases} "
            f"({passed_test_cases / total_test_cases * 100 if total_test_cases > 0 else 0:.1f}%)"
        )
        print(f"Total Execution Time:   {total_time:.3f}s")
        print(f"Average Time per File:  {total_time / total_files:.3f}s")
        print(f"Total NAND Gates:       {total_nand_gates}")
        print(f"Average NAND per File:  {avg_nand_gates:.1f}")
        if self.run_failed and self.run_failure_message:
            print(f"Run Failure:            {self.run_failure_message}")

        failed_files = [report for report in self.reports if not report.success]
        if failed_files:
            print(f"\nFailed Files ({len(failed_files)}):")
            for report in failed_files:
                reason = (
                    "Parse error"
                    if not report.parse_success
                    else "Truth table error"
                    if not report.truth_table_success
                    else "Test failures"
                    if report.has_tests and not report.test_success
                    else "Unknown error"
                )
                print(f"  [FAIL] {report.sv_file} ({reason})")

        print("=" * 60)


def run_simulation(
    file_path: str,
    test_file: Optional[str] = None,
    max_combinations: int = 256,
    clear_cache_first: bool = False,
) -> int:
    """Run single-file simulation mode."""
    if clear_cache_first:
        clear_module_cache()
        print("Global module cache cleared.")

    try:
        print(f"Parsing SystemVerilog file: {file_path}")
        sv_parser = SystemVerilogParser()
        module_info = sv_parser.parse_file(file_path)

        print(f"Module: {module_info['name']}")
        print(f"Inputs: {module_info['inputs']}")
        print(f"Outputs: {module_info['outputs']}")

        evaluator = create_evaluator(
            module_info, filepath=file_path, check_submodules=True
        )

        is_sequential = hasattr(evaluator, "evaluate_cycle")
        if is_sequential:
            print(
                "Sequential blocks detected: "
                f"{len(module_info.get('sequential_blocks', []))}"
            )
            print(f"Clock signals: {list(module_info.get('clock_signals', []))}")

        nand_count = evaluator.count_nand_gates()
        print(f"\nNAND Gate Count: {nand_count}")

        if not is_sequential:
            truth_table_gen = TruthTableGenerator(evaluator)
            truth_table = truth_table_gen.generate_truth_table(max_combinations)
            truth_table_gen.print_truth_table(truth_table)

            image_path = str(Path(file_path).with_suffix(".png"))
            image_gen = TruthTableImageGenerator(evaluator)
            image_gen.generate_image(truth_table, image_path)
            print(f"\nTruth Table Image: {image_path}")
        else:
            print("\nTruth Table: Skipped (sequential logic module)")

        if test_file:
            test_runner = TestRunner(evaluator)
            tests = test_runner.load_tests(test_file)
            passed, total = test_runner.run_tests(tests)

            print(f"\nTest Results: {passed}/{total} passed")

            if is_sequential and test_runner.test_cycles:
                image_path = str(Path(file_path).with_suffix(".png"))
                waveform_gen = WaveformImageGenerator(evaluator)
                waveform_gen.generate_image(test_runner.test_cycles, image_path)
                print(f"Waveform Image: {image_path}")

            if passed == total:
                print("All tests passed!")
            else:
                print(f"{total - passed} test(s) failed.")
                return 1

        return 0
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


def run_test_suite(
    path: str,
    sequential: bool = False,
    workers: Optional[int] = None,
    max_combinations: int = 16,
) -> int:
    """Run batch test mode against one file or a directory tree."""
    if not os.path.exists(path):
        print(f"Error: Path '{path}' does not exist", file=sys.stderr)
        return 1
    if workers is not None and workers < 1:
        print("Error: --workers must be at least 1", file=sys.stderr)
        return 1

    parallel = not sequential and (workers is None or workers > 1)
    runner = SystemVerilogTestRunner(parallel=parallel, max_workers=workers)
    runner.max_combinations = max_combinations

    print("SystemVerilog Test Runner")
    print("=" * 50)
    print(f"Target: {path}")
    print(f"Max combinations: {runner.max_combinations}")
    if parallel:
        print(f"Parallel processing: {runner.max_workers} workers")
    else:
        print("Running sequentially")
    print()

    start_time = time.time()
    runner.run_tests(path)
    end_time = time.time()

    runner.print_summary_report()
    print(f"\nTotal runtime: {end_time - start_time:.3f}s")

    failed_files = sum(1 for report in runner.reports if not report.success)
    if runner.run_failed:
        print(f"\nExiting with code 1 ({runner.run_failure_message})")
        return 1
    if failed_files > 0:
        print(f"\nExiting with code 1 ({failed_files} files failed)")
        return 1

    print("\nAll tests passed! Exiting with code 0")
    return 0


def build_cli_parser() -> argparse.ArgumentParser:
    """Build the consolidated CLI parser."""
    parser = argparse.ArgumentParser(
        description="Single-file SystemVerilog simulator and test runner",
        epilog=(
            "Examples:\n"
            "  python pysvsim.py --file parts/basic/full_adder.sv\n"
            "  python pysvsim.py --file parts/basic/full_adder.sv --test parts/basic/full_adder.json\n"
            "  python pysvsim.py parts/basic/\n"
            "  python pysvsim.py parts/basic/and_gate.sv --sequential"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    parser.add_argument(
        "path",
        nargs="?",
        help="SystemVerilog file or directory to batch test",
    )
    parser.add_argument("--file", help="SystemVerilog file to simulate")
    parser.add_argument("--test", help="JSON test file for single-file simulation")
    parser.add_argument(
        "--max-combinations",
        "-c",
        type=int,
        help="Maximum truth table combinations (default: 256 for --file, 16 for batch mode)",
    )
    parser.add_argument(
        "--clear-cache",
        action="store_true",
        help="Clear the global module cache before single-file simulation",
    )
    parser.add_argument(
        "--sequential",
        "-s",
        action="store_true",
        help="Run batch tests sequentially instead of in parallel",
    )
    parser.add_argument(
        "--workers",
        "-w",
        type=int,
        help="Number of parallel workers for batch mode",
    )
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    """Main entry point for the consolidated single-file module."""
    multiprocessing.freeze_support()
    parser = build_cli_parser()
    args = parser.parse_args(argv)

    if args.file:
        max_combinations = args.max_combinations if args.max_combinations is not None else 256
        return run_simulation(
            file_path=args.file,
            test_file=args.test,
            max_combinations=max_combinations,
            clear_cache_first=args.clear_cache,
        )

    if args.path:
        max_combinations = args.max_combinations if args.max_combinations is not None else 16
        return run_test_suite(
            path=args.path,
            sequential=args.sequential,
            workers=args.workers,
            max_combinations=max_combinations,
        )

    parser.error("provide either --file <sv_file> or a file/directory path to batch test")
    return 2


if __name__ == "__main__":
    sys.exit(main())
