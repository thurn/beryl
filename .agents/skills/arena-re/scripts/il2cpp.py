"""Read-only IL2CPP v39 metadata and Mach-O ARM64 disassembly helpers for inspect_mtga.py."""

from __future__ import annotations

import argparse
import bisect
import re
import struct
import subprocess
import sys
from pathlib import Path


def load_il2cpp_metadata(mtga: Path) -> tuple[bytes, dict[str, tuple[int, int, int]]]:
    meta_path = (
        mtga
        / "MTGA.app/Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat"
    )
    meta = meta_path.read_bytes()
    magic, version = struct.unpack("<II", meta[:8])
    if magic != 0xFAB11BAF:
        raise ValueError(f"Unexpected global-metadata magic 0x{magic:08x}")
    if version != 39:
        print(
            f"# warning: expected IL2CPP metadata v39, found v{version}",
            file=sys.stderr,
        )
    str_off, str_sz = struct.unpack("<II", meta[32:40])
    m_off, m_sz, m_cnt = struct.unpack("<III", meta[68:80])
    f_off, f_sz, f_cnt = struct.unpack("<III", meta[140:152])
    td_off, td_sz, td_cnt = struct.unpack("<III", meta[236:248])
    return meta, {
        "strings": (str_off, str_sz, 0),
        "methods": (m_off, m_sz, m_cnt),
        "fields": (f_off, f_sz, f_cnt),
        "types": (td_off, td_sz, td_cnt),
    }


def meta_string(meta: bytes, str_off: int, str_sz: int, idx: int) -> str:
    if idx < 0 or idx >= str_sz:
        return f"<bad:{idx}>"
    pos = str_off + idx
    end = meta.index(b"\x00", pos)
    return meta[pos:end].decode("utf-8", "replace")


def iter_matching_types(
    meta: bytes, tables: dict[str, tuple[int, int, int]], pattern: re.Pattern[str]
) -> list[tuple[str, list[str], list[tuple[str, int, int]]]]:
    str_off, str_sz, _ = tables["strings"]
    m_off, _, _ = tables["methods"]
    f_off, _, _ = tables["fields"]
    td_off, _, td_cnt = tables["types"]
    out: list[tuple[str, list[str], list[tuple[str, int, int]]]] = []
    for tdi in range(td_cnt):
        base = td_off + tdi * 82
        ni, nsi = struct.unpack("<II", meta[base : base + 8])
        name = meta_string(meta, str_off, str_sz, ni)
        ns = meta_string(meta, str_off, str_sz, nsi)
        full = f"{ns}.{name}" if ns else name
        if not pattern.search(full):
            continue
        fstart, mstart = struct.unpack("<II", meta[base + 26 : base + 34])
        mcnt = struct.unpack("<H", meta[base + 58 : base + 60])[0]
        fcnt = struct.unpack("<H", meta[base + 62 : base + 64])[0]
        fields = [
            meta_string(
                meta,
                str_off,
                str_sz,
                struct.unpack("<I", meta[f_off + fi * 12 : f_off + fi * 12 + 4])[0],
            )
            for fi in range(fstart, fstart + fcnt)
        ]
        methods: list[tuple[str, int, int]] = []
        for mi in range(mstart, mstart + mcnt):
            mni, _, _, _, _, tok, _, _, _, pcnt = struct.unpack(
                "<IIIIIIHHHH", meta[m_off + mi * 32 : m_off + (mi + 1) * 32]
            )
            methods.append((meta_string(meta, str_off, str_sz, mni), pcnt, tok))
        out.append((full, fields, methods))
    return out


def cmd_types(mtga: Path, args: argparse.Namespace) -> None:
    meta, tables = load_il2cpp_metadata(mtga)
    hits = iter_matching_types(meta, tables, re.compile(args.pattern, re.IGNORECASE))
    print(f"# {len(hits)} matching type(s) (showing up to {args.limit})")
    for full, fields, methods in hits[: args.limit]:
        print(f"\n=== {full} (fields={len(fields)}, methods={len(methods)}) ===")
        for f in fields[: args.max_members]:
            print(f"  F: {f}")
        for mname, pcnt, tok in methods[: args.max_members]:
            print(f"  M: {mname} (params={pcnt}, tok=0x{tok:08x})")


def find_arm64_module_methods(ga_path: Path, assembly: str) -> list[int]:
    """Resolves Il2CppCodeGenModule.methodPointers for `assembly` in universal Mach-O."""
    with open(ga_path, "rb") as f:
        raw_hdr = f.read(64)
    magic = struct.unpack(">I", raw_hdr[:4])[0]
    arm64_off = 0
    if magic == 0xCAFEBABE:
        nfat = struct.unpack(">I", raw_hdr[4:8])[0]
        for i in range(nfat):
            cputype, _sub, off, _sz, _align = struct.unpack(
                ">IIIII", raw_hdr[8 + i * 20 : 28 + i * 20]
            )
            if cputype == 0x0100000C:  # CPU_TYPE_ARM64
                arm64_off = off
                break
    otool = subprocess.check_output(
        ["otool", "-arch", "arm64", "-l", str(ga_path)], text=True
    )
    sections: dict[str, tuple[int, int, int]] = {}
    cur_seg = ""
    cur_sect = ""
    addr = size = offset = 0
    for line in otool.splitlines():
        s = line.strip()
        if s.startswith("sectname "):
            cur_sect = s.split()[1]
        elif s.startswith("segname "):
            cur_seg = s.split()[1]
        elif s.startswith("addr "):
            addr = int(s.split()[1], 16)
        elif s.startswith("size "):
            size = int(s.split()[1], 16)
        elif s.startswith("offset "):
            offset = int(s.split()[1])
            if cur_sect:
                sections[f"{cur_seg}.{cur_sect}"] = (addr, size, offset)
                cur_sect = ""
        elif s.startswith("vmaddr "):
            addr = int(s.split()[1], 16)
        elif s.startswith("fileoff "):
            offset = int(s.split()[1])
        elif s.startswith("filesize "):
            size = int(s.split()[1])
            if cur_seg:
                sections[cur_seg] = (addr, size, offset)
    cs_addr, cs_sz, cs_off = sections["__TEXT.__cstring"]
    mask36 = (1 << 36) - 1
    needle = b"\x00" + assembly.encode("ascii") + b"\x00"
    with open(ga_path, "rb") as f:
        f.seek(arm64_off + cs_off)
        cstr = f.read(cs_sz)
        vas: set[int] = set()
        pos = 0
        while True:
            idx = cstr.find(needle, pos)
            if idx == -1:
                break
            vas.add(cs_addr + idx + 1)
            pos = idx + 1
        if not vas:
            raise ValueError(
                f"Assembly name {assembly!r} not found in __TEXT.__cstring"
            )
        best: tuple[int, int] | None = None
        for seg_name in ("__DATA_CONST", "__DATA"):
            if seg_name not in sections:
                continue
            _sa, seg_sz, seg_off = sections[seg_name]
            f.seek(arm64_off + seg_off)
            buf = f.read(seg_sz)
            for off in range(0, len(buf) - 24, 8):
                val = struct.unpack("<Q", buf[off : off + 8])[0]
                if (val & mask36) in vas:
                    mp_cnt = struct.unpack("<I", buf[off + 8 : off + 12])[0]
                    mp_ptr = struct.unpack("<Q", buf[off + 16 : off + 24])[0] & mask36
                    if 10 < mp_cnt < 500_000 and mp_ptr > 0:
                        if best is None or mp_cnt > best[0]:
                            best = (mp_cnt, mp_ptr)
        if best is None:
            raise ValueError(f"Il2CppCodeGenModule for {assembly!r} not found")
        mp_cnt, mp_ptr = best
        mp_file_off = mp_ptr
        for seg_name in ("__DATA_CONST", "__DATA"):
            if seg_name in sections:
                sa, ssz, soff = sections[seg_name]
                if sa <= mp_ptr < sa + ssz:
                    mp_file_off = soff + (mp_ptr - sa)
                    break
        f.seek(arm64_off + mp_file_off)
        raw = f.read(mp_cnt * 8)
    return [
        struct.unpack("<Q", raw[i * 8 : (i + 1) * 8])[0] & mask36 for i in range(mp_cnt)
    ]


def cmd_disasm(mtga: Path, args: argparse.Namespace) -> None:
    meta, tables = load_il2cpp_metadata(mtga)
    type_re = re.compile(args.type_pattern, re.IGNORECASE)
    method_re = re.compile(args.method_pattern, re.IGNORECASE)
    targets: list[tuple[str, int]] = []
    for full, _fields, methods in iter_matching_types(meta, tables, type_re):
        for mname, pcnt, tok in methods:
            if method_re.search(mname):
                targets.append((f"{full}.{mname}({pcnt})", tok))
    if not targets:
        print("# no matching methods")
        return
    ga_path = mtga / "MTGA.app/Contents/Frameworks/GameAssembly.dylib"
    mps = find_arm64_module_methods(ga_path, args.assembly)
    sorted_vas = sorted(set(v for v in mps if v > 0))
    resolved: list[tuple[str, int, int, int]] = []
    for label, tok in targets[: args.limit]:
        idx = (tok & 0x00FFFFFF) - 1
        if idx < 0 or idx >= len(mps) or mps[idx] == 0:
            print(f"# skip {label} (tok=0x{tok:08x} outside {args.assembly})")
            continue
        va = mps[idx]
        pos = bisect.bisect_right(sorted_vas, va)
        end = sorted_vas[pos] if pos < len(sorted_vas) else va + args.max_bytes
        end = min(end, va + args.max_bytes)
        resolved.append((label, tok, va, end))
    if not resolved:
        return
    cmd = ["xcrun", "lldb", "-b", "-o", f'target create -a arm64 "{ga_path}"']
    for _label, _tok, va, end in resolved:
        cmd += ["-o", f"disassemble -s 0x{va:x} -e 0x{end:x}"]
    out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)
    chunks = out.split("(lldb) disassemble ")
    for (label, tok, va, end), chunk in zip(resolved, chunks[1:]):
        print(
            f"\n=== {label} tok=0x{tok:08x} VA=0x{va:x}..0x{end:x} ({end - va} B) ==="
        )
        lines = [
            line
            for line in chunk.splitlines()[1:]
            if line.strip() and not line.startswith("GameAssembly.dylib`")
        ]
        print("\n".join(lines[: args.max_lines]))
        if len(lines) > args.max_lines:
            print(f"  ... ({len(lines) - args.max_lines} more lines)")
