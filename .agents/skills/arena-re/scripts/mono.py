"""Read-only ECMA-335 .NET metadata and CIL disassembly helpers for Windows MTGA."""

from __future__ import annotations

import argparse
import re
import struct
from pathlib import Path

# Single-byte CIL opcodes: opcode -> (mnemonic, operand_kind)
# operand_kind: none, i1, u1, i2, i4, i8, r4, r8, tok, br1, br4, switch
OPCODES_1: dict[int, tuple[str, str]] = {
    0x00: ("nop", "none"),
    0x01: ("break", "none"),
    0x02: ("ldarg.0", "none"),
    0x03: ("ldarg.1", "none"),
    0x04: ("ldarg.2", "none"),
    0x05: ("ldarg.3", "none"),
    0x06: ("ldloc.0", "none"),
    0x07: ("ldloc.1", "none"),
    0x08: ("ldloc.2", "none"),
    0x09: ("ldloc.3", "none"),
    0x0A: ("stloc.0", "none"),
    0x0B: ("stloc.1", "none"),
    0x0C: ("stloc.2", "none"),
    0x0D: ("stloc.3", "none"),
    0x0E: ("ldarg.s", "u1"),
    0x0F: ("ldarga.s", "u1"),
    0x10: ("starg.s", "u1"),
    0x11: ("ldloc.s", "u1"),
    0x12: ("ldloca.s", "u1"),
    0x13: ("stloc.s", "u1"),
    0x14: ("ldnull", "none"),
    0x15: ("ldc.i4.m1", "none"),
    0x16: ("ldc.i4.0", "none"),
    0x17: ("ldc.i4.1", "none"),
    0x18: ("ldc.i4.2", "none"),
    0x19: ("ldc.i4.3", "none"),
    0x1A: ("ldc.i4.4", "none"),
    0x1B: ("ldc.i4.5", "none"),
    0x1C: ("ldc.i4.6", "none"),
    0x1D: ("ldc.i4.7", "none"),
    0x1E: ("ldc.i4.8", "none"),
    0x1F: ("ldc.i4.s", "i1"),
    0x20: ("ldc.i4", "i4"),
    0x21: ("ldc.i8", "i8"),
    0x22: ("ldc.r4", "r4"),
    0x23: ("ldc.r8", "r8"),
    0x25: ("dup", "none"),
    0x26: ("pop", "none"),
    0x27: ("jmp", "tok"),
    0x28: ("call", "tok"),
    0x29: ("calli", "tok"),
    0x2A: ("ret", "none"),
    0x2B: ("br.s", "br1"),
    0x2C: ("brfalse.s", "br1"),
    0x2D: ("brtrue.s", "br1"),
    0x2E: ("beq.s", "br1"),
    0x2F: ("bge.s", "br1"),
    0x30: ("bgt.s", "br1"),
    0x31: ("ble.s", "br1"),
    0x32: ("blt.s", "br1"),
    0x33: ("bne.un.s", "br1"),
    0x34: ("bge.un.s", "br1"),
    0x35: ("bgt.un.s", "br1"),
    0x36: ("ble.un.s", "br1"),
    0x37: ("blt.un.s", "br1"),
    0x38: ("br", "br4"),
    0x39: ("brfalse", "br4"),
    0x3A: ("brtrue", "br4"),
    0x3B: ("beq", "br4"),
    0x3C: ("bge", "br4"),
    0x3D: ("bgt", "br4"),
    0x3E: ("ble", "br4"),
    0x3F: ("blt", "br4"),
    0x40: ("bne.un", "br4"),
    0x41: ("bge.un", "br4"),
    0x42: ("bgt.un", "br4"),
    0x43: ("ble.un", "br4"),
    0x44: ("blt.un", "br4"),
    0x45: ("switch", "switch"),
    0x46: ("ldind.i1", "none"),
    0x47: ("ldind.u1", "none"),
    0x48: ("ldind.i2", "none"),
    0x49: ("ldind.u2", "none"),
    0x4A: ("ldind.i4", "none"),
    0x4B: ("ldind.u4", "none"),
    0x4C: ("ldind.i8", "none"),
    0x4D: ("ldind.i", "none"),
    0x4E: ("ldind.r4", "none"),
    0x4F: ("ldind.r8", "none"),
    0x50: ("ldind.ref", "none"),
    0x51: ("stind.ref", "none"),
    0x52: ("stind.i1", "none"),
    0x53: ("stind.i2", "none"),
    0x54: ("stind.i4", "none"),
    0x55: ("stind.i8", "none"),
    0x56: ("stind.r4", "none"),
    0x57: ("stind.r8", "none"),
    0x58: ("add", "none"),
    0x59: ("sub", "none"),
    0x5A: ("mul", "none"),
    0x5B: ("div", "none"),
    0x5C: ("div.un", "none"),
    0x5D: ("rem", "none"),
    0x5E: ("rem.un", "none"),
    0x5F: ("and", "none"),
    0x60: ("or", "none"),
    0x61: ("xor", "none"),
    0x62: ("shl", "none"),
    0x63: ("shr", "none"),
    0x64: ("shr.un", "none"),
    0x65: ("neg", "none"),
    0x66: ("not", "none"),
    0x67: ("conv.i1", "none"),
    0x68: ("conv.i2", "none"),
    0x69: ("conv.i4", "none"),
    0x6A: ("conv.i8", "none"),
    0x6B: ("conv.r4", "none"),
    0x6C: ("conv.r8", "none"),
    0x6D: ("conv.u4", "none"),
    0x6E: ("conv.u8", "none"),
    0x6F: ("callvirt", "tok"),
    0x70: ("cpobj", "tok"),
    0x71: ("ldobj", "tok"),
    0x72: ("ldstr", "tok"),
    0x73: ("newobj", "tok"),
    0x74: ("castclass", "tok"),
    0x75: ("isinst", "tok"),
    0x76: ("conv.r.un", "none"),
    0x79: ("unbox", "tok"),
    0x7A: ("throw", "none"),
    0x7B: ("ldfld", "tok"),
    0x7C: ("ldflda", "tok"),
    0x7D: ("stfld", "tok"),
    0x7E: ("ldsfld", "tok"),
    0x7F: ("ldsflda", "tok"),
    0x80: ("stsfld", "tok"),
    0x81: ("stobj", "tok"),
    0x8C: ("box", "tok"),
    0x8D: ("newarr", "tok"),
    0x8E: ("ldlen", "none"),
    0x8F: ("ldelema", "tok"),
    0x90: ("ldelem.i1", "none"),
    0x91: ("ldelem.u1", "none"),
    0x92: ("ldelem.i2", "none"),
    0x93: ("ldelem.u2", "none"),
    0x94: ("ldelem.i4", "none"),
    0x95: ("ldelem.u4", "none"),
    0x96: ("ldelem.i8", "none"),
    0x97: ("ldelem.i", "none"),
    0x98: ("ldelem.r4", "none"),
    0x99: ("ldelem.r8", "none"),
    0x9A: ("ldelem.ref", "none"),
    0x9B: ("stelem.i", "none"),
    0x9C: ("stelem.i1", "none"),
    0x9D: ("stelem.i2", "none"),
    0x9E: ("stelem.i4", "none"),
    0x9F: ("stelem.i8", "none"),
    0xA0: ("stelem.r4", "none"),
    0xA1: ("stelem.r8", "none"),
    0xA2: ("stelem.ref", "none"),
    0xA3: ("ldelem", "tok"),
    0xA4: ("stelem", "tok"),
    0xA5: ("unbox.any", "tok"),
    0xD0: ("ldtoken", "tok"),
    0xD1: ("conv.u2", "none"),
    0xD2: ("conv.u1", "none"),
    0xD3: ("conv.i", "none"),
    0xDC: ("endfinally", "none"),
    0xDD: ("leave", "br4"),
    0xDE: ("leave.s", "br1"),
    0xDF: ("stind.i", "none"),
    0xE0: ("conv.u", "none"),
}

OPCODES_FE: dict[int, tuple[str, str]] = {
    0x00: ("arglist", "none"),
    0x01: ("ceq", "none"),
    0x02: ("cgt", "none"),
    0x03: ("cgt.un", "none"),
    0x04: ("clt", "none"),
    0x05: ("clt.un", "none"),
    0x06: ("ldftn", "tok"),
    0x07: ("ldvirtftn", "tok"),
    0x09: ("ldarg", "u2"),
    0x0A: ("ldarga", "u2"),
    0x0B: ("starg", "u2"),
    0x0C: ("ldloc", "u2"),
    0x0D: ("ldloca", "u2"),
    0x0E: ("stloc", "u2"),
    0x15: ("initobj", "tok"),
    0x16: ("constrained.", "tok"),
    0x1A: ("rethrow", "none"),
    0x1C: ("sizeof", "tok"),
}


class ManagedAssembly:
    """Minimal ECMA-335 PE/CLI metadata reader for unobfuscated Mono assemblies."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.data = path.read_bytes()
        self.sections = self._parse_sections()
        cli_rva = self._cli_header_rva()
        cli_off = self.rva_to_off(cli_rva)
        meta_rva, _meta_sz = struct.unpack("<II", self.data[cli_off + 8 : cli_off + 16])
        self.meta_off = self.rva_to_off(meta_rva)
        self.streams = self._parse_streams()
        self.strings = self.streams.get("#Strings", b"")
        self.us = self.streams.get("#US", b"")
        self.blob = self.streams.get("#Blob", b"")
        self._parse_tables()

    def _parse_sections(self) -> list[tuple[int, int, int]]:
        pe = struct.unpack("<I", self.data[0x3C:0x40])[0]
        if self.data[pe : pe + 4] != b"PE\x00\x00":
            raise ValueError(f"{self.path.name}: not a PE file")
        nsec = struct.unpack("<H", self.data[pe + 6 : pe + 8])[0]
        opt_sz = struct.unpack("<H", self.data[pe + 20 : pe + 22])[0]
        sec_off = pe + 24 + opt_sz
        out: list[tuple[int, int, int]] = []
        for i in range(nsec):
            s = sec_off + i * 40
            vsz, va, rsz, raw = struct.unpack("<IIII", self.data[s + 8 : s + 24])
            out.append((va, max(vsz, rsz), raw))
        return out

    def _cli_header_rva(self) -> int:
        pe = struct.unpack("<I", self.data[0x3C:0x40])[0]
        magic = struct.unpack("<H", self.data[pe + 24 : pe + 26])[0]
        dd_off = pe + 24 + (112 if magic == 0x20B else 96)
        return struct.unpack("<I", self.data[dd_off + 14 * 8 : dd_off + 14 * 8 + 4])[0]

    def rva_to_off(self, rva: int) -> int:
        for va, sz, raw in self.sections:
            if va <= rva < va + sz:
                return raw + (rva - va)
        raise ValueError(f"{self.path.name}: RVA 0x{rva:x} outside PE sections")

    def _parse_streams(self) -> dict[str, bytes]:
        m = self.meta_off
        if self.data[m : m + 4] != b"BSJB":
            raise ValueError(f"{self.path.name}: missing BSJB metadata header")
        vlen = struct.unpack("<I", self.data[m + 12 : m + 16])[0]
        pos = m + 16 + vlen + 2
        nstreams = struct.unpack("<H", self.data[pos : pos + 2])[0]
        pos += 2
        streams: dict[str, bytes] = {}
        for _ in range(nstreams):
            off, sz = struct.unpack("<II", self.data[pos : pos + 8])
            pos += 8
            end = self.data.index(b"\x00", pos)
            name = self.data[pos:end].decode("ascii", "replace")
            pos = ((end + 1) + 3) & ~3
            streams[name] = self.data[m + off : m + off + sz]
        return streams

    def get_str(self, idx: int) -> str:
        if idx <= 0 or idx >= len(self.strings):
            return ""
        end = self.strings.find(b"\x00", idx)
        if end == -1:
            end = len(self.strings)
        return self.strings[idx:end].decode("utf-8", "replace")

    def get_us(self, idx: int) -> str:
        if idx <= 0 or idx >= len(self.us):
            return ""
        b0 = self.us[idx]
        if (b0 & 0x80) == 0:
            ln, pos = b0, idx + 1
        elif (b0 & 0xC0) == 0x80:
            ln, pos = ((b0 & 0x3F) << 8) | self.us[idx + 1], idx + 2
        else:
            ln = (
                ((b0 & 0x1F) << 24)
                | (self.us[idx + 1] << 16)
                | (self.us[idx + 2] << 8)
                | self.us[idx + 3]
            )
            pos = idx + 4
        if ln <= 1:
            return ""
        return self.us[pos : pos + ln - 1].decode("utf-16-le", "replace")

    def _parse_tables(self) -> None:
        buf = self.streams.get("#~") or self.streams.get("#-")
        if not buf:
            raise ValueError(f"{self.path.name}: missing #~ metadata stream")
        heap_sizes = buf[6]
        valid = struct.unpack("<Q", buf[8:16])[0]
        pos = 24
        counts: dict[int, int] = {}
        for tid in range(64):
            if (valid >> tid) & 1:
                counts[tid] = struct.unpack("<I", buf[pos : pos + 4])[0]
                pos += 4

        str_sz = 4 if (heap_sizes & 1) else 2
        guid_sz = 4 if (heap_sizes & 2) else 2
        blob_sz = 4 if (heap_sizes & 4) else 2

        def tbl_sz(tid: int) -> int:
            return 4 if counts.get(tid, 0) >= 65536 else 2

        def coded_sz(bits: int, tids: tuple[int, ...]) -> int:
            limit = 1 << (16 - bits)
            return 4 if any(counts.get(t, 0) >= limit for t in tids) else 2

        def read_u(p: int, sz: int) -> tuple[int, int]:
            return (
                struct.unpack("<I", buf[p : p + 4])[0]
                if sz == 4
                else struct.unpack("<H", buf[p : p + 2])[0]
            ), p + sz

        rs_sz = coded_sz(2, (0x00, 0x1A, 0x23, 0x01))
        tdr_sz = coded_sz(2, (0x02, 0x01, 0x1B))
        mrp_sz = coded_sz(3, (0x02, 0x01, 0x1A, 0x06, 0x1B))
        f_idx_sz = tbl_sz(0x04)
        m_idx_sz = tbl_sz(0x06)
        p_idx_sz = tbl_sz(0x08)
        td_idx_sz = tbl_sz(0x02)

        self.typerefs: list[tuple[str, str]] = []
        self.typedefs: list[tuple[str, str, int, int]] = []
        self.fields: list[str] = []
        self.methods: list[tuple[int, str, int]] = []
        self.memberrefs: list[tuple[int, str]] = []

        # 0x00 Module
        pos += counts.get(0x00, 0) * (2 + str_sz + guid_sz * 3)
        # 0x01 TypeRef
        for _ in range(counts.get(0x01, 0)):
            pos += rs_sz
            ni, pos = read_u(pos, str_sz)
            nsi, pos = read_u(pos, str_sz)
            self.typerefs.append((self.get_str(nsi), self.get_str(ni)))
        # 0x02 TypeDef
        for _ in range(counts.get(0x02, 0)):
            pos += 4
            ni, pos = read_u(pos, str_sz)
            nsi, pos = read_u(pos, str_sz)
            pos += tdr_sz
            flist, pos = read_u(pos, f_idx_sz)
            mlist, pos = read_u(pos, m_idx_sz)
            self.typedefs.append((self.get_str(nsi), self.get_str(ni), flist, mlist))
        # 0x03 FieldPtr
        pos += counts.get(0x03, 0) * f_idx_sz
        # 0x04 Field
        for _ in range(counts.get(0x04, 0)):
            pos += 2
            ni, pos = read_u(pos, str_sz)
            pos += blob_sz
            self.fields.append(self.get_str(ni))
        # 0x05 MethodPtr
        pos += counts.get(0x05, 0) * m_idx_sz
        # 0x06 MethodDef
        for _ in range(counts.get(0x06, 0)):
            rva = struct.unpack("<I", buf[pos : pos + 4])[0]
            pos += 8
            ni, pos = read_u(pos, str_sz)
            sig_idx, pos = read_u(pos, blob_sz)
            pos += p_idx_sz
            self.methods.append((rva, self.get_str(ni), self._sig_param_count(sig_idx)))
        # 0x07 ParamPtr, 0x08 Param, 0x09 InterfaceImpl
        pos += counts.get(0x07, 0) * p_idx_sz
        pos += counts.get(0x08, 0) * (4 + str_sz)
        pos += counts.get(0x09, 0) * (td_idx_sz + tdr_sz)
        # 0x0A MemberRef
        for _ in range(counts.get(0x0A, 0)):
            cls, pos = read_u(pos, mrp_sz)
            ni, pos = read_u(pos, str_sz)
            pos += blob_sz
            self.memberrefs.append((cls, self.get_str(ni)))

        # Map field/method 1-based row index -> owning type name
        self.field_owner: dict[int, str] = {}
        self.method_owner: dict[int, str] = {}
        ntd = len(self.typedefs)
        for ti, (ns, name, flist, mlist) in enumerate(self.typedefs):
            full = f"{ns}.{name}" if ns else name
            fend = self.typedefs[ti + 1][2] if ti + 1 < ntd else len(self.fields) + 1
            mend = self.typedefs[ti + 1][3] if ti + 1 < ntd else len(self.methods) + 1
            for fi in range(flist, fend):
                self.field_owner[fi] = full
            for mi in range(mlist, mend):
                self.method_owner[mi] = full

    def _sig_param_count(self, blob_idx: int) -> int:
        if blob_idx <= 0 or blob_idx >= len(self.blob):
            return 0
        b0 = self.blob[blob_idx]
        pos = blob_idx + (1 if (b0 & 0x80) == 0 else 2 if (b0 & 0xC0) == 0x80 else 4)
        if pos + 1 >= len(self.blob):
            return 0
        calling = self.blob[pos]
        pos += 1
        if calling & 0x10:  # GENERIC: skip GenParamCount
            pos += 1
        return self.blob[pos] if pos < len(self.blob) else 0

    def resolve_token(self, tok: int) -> str:
        tbl = tok >> 24
        rid = tok & 0x00FFFFFF
        if tbl == 0x70:
            return repr(self.get_us(rid))
        if rid == 0:
            return f"0x{tok:08x}"
        if tbl == 0x01 and rid <= len(self.typerefs):
            ns, n = self.typerefs[rid - 1]
            return f"{ns}.{n}" if ns else n
        if tbl == 0x02 and rid <= len(self.typedefs):
            ns, n, _, _ = self.typedefs[rid - 1]
            return f"{ns}.{n}" if ns else n
        if tbl == 0x04 and rid <= len(self.fields):
            owner = self.field_owner.get(rid, "")
            return f"{owner}::{self.fields[rid - 1]}" if owner else self.fields[rid - 1]
        if tbl == 0x06 and rid <= len(self.methods):
            owner = self.method_owner.get(rid, "")
            mname = self.methods[rid - 1][1]
            return f"{owner}::{mname}" if owner else mname
        if tbl == 0x0A and rid <= len(self.memberrefs):
            cls, mname = self.memberrefs[rid - 1]
            tag, idx = cls & 7, cls >> 3
            cname = ""
            if tag == 0 and 1 <= idx <= len(self.typedefs):
                ns, n, _, _ = self.typedefs[idx - 1]
                cname = f"{ns}.{n}" if ns else n
            elif tag == 1 and 1 <= idx <= len(self.typerefs):
                ns, n = self.typerefs[idx - 1]
                cname = f"{ns}.{n}" if ns else n
            return f"{cname}::{mname}" if cname else mname
        return f"0x{tok:08x}"

    def iter_matching_types(
        self, pattern: re.Pattern[str]
    ) -> list[tuple[str, list[str], list[tuple[str, int, int, int]]]]:
        out: list[tuple[str, list[str], list[tuple[str, int, int, int]]]] = []
        ntd = len(self.typedefs)
        for ti, (ns, name, flist, mlist) in enumerate(self.typedefs):
            full = f"{ns}.{name}" if ns else name
            if not pattern.search(full):
                continue
            fend = self.typedefs[ti + 1][2] if ti + 1 < ntd else len(self.fields) + 1
            mend = self.typedefs[ti + 1][3] if ti + 1 < ntd else len(self.methods) + 1
            fields = [
                self.fields[fi - 1]
                for fi in range(flist, fend)
                if 1 <= fi <= len(self.fields)
            ]
            methods = [
                (
                    self.methods[mi - 1][1],
                    self.methods[mi - 1][2],
                    0x06000000 | mi,
                    self.methods[mi - 1][0],
                )
                for mi in range(mlist, mend)
                if 1 <= mi <= len(self.methods)
            ]
            out.append((full, fields, methods))
        return out

    def disassemble_method(self, rva: int, max_bytes: int) -> list[str]:
        if rva == 0:
            return ["  <abstract or extern method (RVA=0)>"]
        off = self.rva_to_off(rva)
        b0 = self.data[off]
        if (b0 & 3) == 2:  # Tiny header
            code_sz = b0 >> 2
            code_off = off + 1
        elif (b0 & 3) == 3:  # Fat header
            hdr_dwords = self.data[off + 1] >> 4
            code_sz = struct.unpack("<I", self.data[off + 4 : off + 8])[0]
            code_off = off + hdr_dwords * 4
        else:
            return [f"  <unrecognized CIL header 0x{b0:02x}>"]
        code = self.data[code_off : code_off + min(code_sz, max_bytes)]
        lines: list[str] = []
        i = 0
        n = len(code)
        while i < n:
            il_off = i
            op = code[i]
            i += 1
            if op == 0xFE and i < n:
                op2 = code[i]
                i += 1
                mnem, kind = OPCODES_FE.get(op2, (f"fe.{op2:02x}", "none"))
            else:
                mnem, kind = OPCODES_1.get(op, (f"op_{op:02x}", "none"))
            arg = ""
            if kind == "u1" and i + 1 <= n:
                arg = str(code[i])
                i += 1
            elif kind == "i1" and i + 1 <= n:
                arg = str(struct.unpack("<b", code[i : i + 1])[0])
                i += 1
            elif kind == "u2" and i + 2 <= n:
                arg = str(struct.unpack("<H", code[i : i + 2])[0])
                i += 2
            elif kind == "i4" and i + 4 <= n:
                arg = str(struct.unpack("<i", code[i : i + 4])[0])
                i += 4
            elif kind == "i8" and i + 8 <= n:
                arg = str(struct.unpack("<q", code[i : i + 8])[0])
                i += 8
            elif kind == "r4" and i + 4 <= n:
                arg = f"{struct.unpack('<f', code[i : i + 4])[0]:g}"
                i += 4
            elif kind == "r8" and i + 8 <= n:
                arg = f"{struct.unpack('<d', code[i : i + 8])[0]:g}"
                i += 8
            elif kind == "br1" and i + 1 <= n:
                rel = struct.unpack("<b", code[i : i + 1])[0]
                i += 1
                arg = f"IL_{i + rel:04x}"
            elif kind == "br4" and i + 4 <= n:
                rel = struct.unpack("<i", code[i : i + 4])[0]
                i += 4
                arg = f"IL_{i + rel:04x}"
            elif kind == "tok" and i + 4 <= n:
                tok = struct.unpack("<I", code[i : i + 4])[0]
                i += 4
                arg = self.resolve_token(tok)
            elif kind == "switch" and i + 4 <= n:
                cnt = struct.unpack("<I", code[i : i + 4])[0]
                i += 4
                base = i + cnt * 4
                targets = []
                for _ in range(min(cnt, 8)):
                    if i + 4 > n:
                        break
                    rel = struct.unpack("<i", code[i : i + 4])[0]
                    targets.append(f"IL_{base + rel:04x}")
                    i += 4
                i = base
                arg = f"({', '.join(targets)}{', ...' if cnt > 8 else ''})"
            lines.append(f"  IL_{il_off:04x}: {mnem:14s} {arg}".rstrip())
        return lines


def managed_dlls(mtga: Path, assembly: str | None = None) -> list[Path]:
    mdir = mtga / "MTGA_Data/Managed"
    if assembly:
        p = mdir / assembly
        if not p.is_file():
            raise FileNotFoundError(f"Managed assembly not found: {p}")
        return [p]
    preferred = [mdir / "Core.dll", mdir / "Assembly-CSharp.dll"]
    wizards = sorted(mdir.glob("Wizards.*.dll"))
    return [p for p in preferred + wizards if p.is_file()]


def cmd_mono_symbols(mtga: Path, args: argparse.Namespace) -> None:
    pattern = re.compile(args.pattern, re.IGNORECASE)
    dlls = managed_dlls(mtga)
    matches: set[str] = set()
    for p in dlls:
        data = p.read_bytes()
        for m in re.finditer(rb"[A-Za-z_][A-Za-z0-9_]{3,90}", data):
            s = m.group(0).decode("ascii")
            if pattern.search(s):
                matches.add(s)
    ordered = sorted(matches)
    print(f"# {len(dlls)} managed DLL(s) in {mtga / 'MTGA_Data/Managed'}")
    print(f"# {len(ordered)} matches (showing up to {args.limit}):")
    for s in ordered[: args.limit]:
        print(f"  {s}")


def cmd_mono_types(mtga: Path, args: argparse.Namespace) -> None:
    asm = ManagedAssembly(managed_dlls(mtga, getattr(args, "assembly", "Core.dll"))[0])
    hits = asm.iter_matching_types(re.compile(args.pattern, re.IGNORECASE))
    print(
        f"# {asm.path.name}: {len(hits)} matching type(s) (showing up to {args.limit})"
    )
    for full, fields, methods in hits[: args.limit]:
        print(f"\n=== {full} (fields={len(fields)}, methods={len(methods)}) ===")
        for f in fields[: args.max_members]:
            print(f"  F: {f}")
        for mname, pcnt, tok, _rva in methods[: args.max_members]:
            print(f"  M: {mname} (params={pcnt}, tok=0x{tok:08x})")


def cmd_mono_disasm(mtga: Path, args: argparse.Namespace) -> None:
    asm = ManagedAssembly(managed_dlls(mtga, args.assembly)[0])
    type_re = re.compile(args.type_pattern, re.IGNORECASE)
    method_re = re.compile(args.method_pattern, re.IGNORECASE)
    targets: list[tuple[str, int, int]] = []
    for full, _fields, methods in asm.iter_matching_types(type_re):
        for mname, pcnt, tok, rva in methods:
            if method_re.search(mname):
                targets.append((f"{full}.{mname}({pcnt})", tok, rva))
    if not targets:
        print("# no matching methods")
        return
    for label, tok, rva in targets[: args.limit]:
        lines = asm.disassemble_method(rva, args.max_bytes)
        print(f"\n=== {label} tok=0x{tok:08x} RVA=0x{rva:x} ({asm.path.name} CIL) ===")
        print("\n".join(lines[: args.max_lines]))
        if len(lines) > args.max_lines:
            print(f"  ... ({len(lines) - args.max_lines} more lines)")
