#!/usr/bin/env python3
"""Read-only inspector for the locally installed macOS or Windows MTG Arena client.

Supports clean-room VFX and animation research by inspecting:
  - Manifest_*.mtga JSON entries (mapping bundle names to Unity Assets/... paths)
  - Asset Lookup Trees (MTGA_Data/Downloads/ALT/ALT_*.mtga plain-text JSON trees)
  - UnityFS v8 (LZ4HC) AssetBundle unpacking (pure stdlib, no external packages)
  - Prefab component/state strings and external CAB-* dependency resolution
  - Unity Material serialized colors, floats, and texture transform scales
  - Plain-text shaders inside Shader_* bundles (MSL on macOS, GLSL on Windows)
  - Unobfuscated C# identifiers, types/fields/methods, and method disassembly
    (IL2CPP global-metadata.dat + ARM64 GameAssembly.dylib on macOS;
    Mono MTGA_Data/Managed/Core.dll ECMA-335 metadata + CIL on Windows)

Strictly read-only. Never exports or copies proprietary textures, meshes, or shaders.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import struct
import sys
from pathlib import Path
from typing import Any

from il2cpp import cmd_disasm as cmd_il2cpp_disasm
from il2cpp import cmd_types as cmd_il2cpp_types
from mono import cmd_mono_disasm, cmd_mono_symbols, cmd_mono_types

DEFAULT_MTGA = (
    Path(r"C:\Program Files (x86)\Steam\steamapps\common\MTGA")
    if sys.platform == "win32"
    else Path.home() / "Library/Application Support/Steam/steamapps/common/MTGA"
)

MATERIAL_PROPS = [
    b"_TintColor",
    b"_HighRangeColorMult",
    b"_Color",
    b"_Color2",
    b"_rimColor",
    b"_LightColor",
    b"_LUTLow",
    b"_LUTHigh",
    b"_LUTIndex",
    b"_LUTAlpha",
    b"_UseLUT",
    b"_AlphaMult",
    b"_Brightness_Lower",
    b"_Brightness_Upper",
    b"_DistTex_PanX",
    b"_DistTex_PanY",
    b"_DistTexSpiral",
    b"_DistTexNormal",
    b"_AlphaMaskDistSTR",
    b"_MainTex_PanX",
    b"_MainTex_PanY",
    b"_MultTex_PanX",
    b"_MultTex_PanY",
    b"_FresnelExp",
    b"_FresnelMult",
    b"_rimPower",
    b"_rimStrength",
    b"_BlendPWR",
    b"_PWR",
    b"_PWR2",
    b"_MLT",
    b"_MLTH",
    b"_MLT2",
    b"_SrcBlend",
    b"_DstBlend",
]

MATERIAL_TEX_PROPS = [
    b"_MainTex",
    b"_DistTex",
    b"_MultTex",
    b"_AlphaMask",
    b"_DistMask",
    b"_LUT",
    b"_Texture",
    b"_Noise",
]


def lz4_decompress(src: bytes) -> bytes:
    """Decompresses a raw LZ4 block using only the Python standard library."""
    out = bytearray()
    i = 0
    n = len(src)
    while i < n:
        token = src[i]
        i += 1
        lit_len = token >> 4
        if lit_len == 15:
            while i < n:
                extra = src[i]
                i += 1
                lit_len += extra
                if extra != 255:
                    break
        out.extend(src[i : i + lit_len])
        i += lit_len
        if i >= n:
            break
        offset = src[i] | (src[i + 1] << 8)
        i += 2
        if offset == 0 or offset > len(out):
            raise ValueError(f"invalid LZ4 back-reference offset {offset}")
        match_len = (token & 0x0F) + 4
        if (token & 0x0F) == 15:
            while i < n:
                extra = src[i]
                i += 1
                match_len += extra
                if extra != 255:
                    break
        start = len(out) - offset
        if offset >= match_len:
            out.extend(out[start : start + match_len])
        else:
            for k in range(match_len):
                out.append(out[start + k])
    return bytes(out)


def read_unityfs_header(
    path: Path,
) -> tuple[list[tuple[int, int, int, str]], bytes, int, list[tuple[int, int, int]]]:
    """Parses a UnityFS container header and returns nodes, raw bytes, data offset, blocks."""
    raw = path.read_bytes()
    pos = 0
    sig = raw.split(b"\x00", 1)[0]
    if sig != b"UnityFS":
        raise ValueError(f"{path.name}: expected UnityFS, found {sig!r}")
    pos += len(sig) + 1
    ver = struct.unpack(">I", raw[pos : pos + 4])[0]
    pos += 4
    u_ver = raw[pos:].split(b"\x00", 1)[0]
    pos += len(u_ver) + 1
    u_rev = raw[pos:].split(b"\x00", 1)[0]
    pos += len(u_rev) + 1
    _size, c_info_sz, _u_info_sz, flags = struct.unpack(">QIII", raw[pos : pos + 20])
    pos += 20
    if ver >= 7:
        pos = (pos + 15) & ~15
    if flags & 0x80:
        info_raw = raw[len(raw) - c_info_sz :]
        data_start = pos
    else:
        info_raw = raw[pos : pos + c_info_sz]
        data_start = pos + c_info_sz
    if flags & 0x200:
        data_start = (data_start + 15) & ~15
    comp = flags & 0x3F
    if comp in (2, 3):
        info = lz4_decompress(info_raw)
    elif comp == 0:
        info = info_raw
    else:
        raise ValueError(f"{path.name}: unsupported blocksInfo compression {comp}")
    ip = 16  # skip 16-byte uncompressed data hash
    block_count = struct.unpack(">I", info[ip : ip + 4])[0]
    ip += 4
    blocks: list[tuple[int, int, int]] = []
    for _ in range(block_count):
        us, cs, bf = struct.unpack(">IIH", info[ip : ip + 10])
        ip += 10
        blocks.append((us, cs, bf))
    node_count = struct.unpack(">I", info[ip : ip + 4])[0]
    ip += 4
    nodes: list[tuple[int, int, int, str]] = []
    for _ in range(node_count):
        off, sz, nf = struct.unpack(">QQI", info[ip : ip + 20])
        ip += 20
        name = info[ip:].split(b"\x00", 1)[0]
        ip += len(name) + 1
        nodes.append((off, sz, nf, name.decode("utf-8", "replace")))
    return nodes, raw, data_start, blocks


def unpack_unityfs(path: Path) -> tuple[list[tuple[int, int, int, str]], bytes]:
    """Decompresses all blocks of a UnityFS AssetBundle into memory."""
    nodes, raw, data_start, blocks = read_unityfs_header(path)
    blob = bytearray()
    dp = data_start
    for _us, cs, bf in blocks:
        chunk = raw[dp : dp + cs]
        dp += cs
        bc = bf & 0x3F
        if bc in (2, 3):
            blob.extend(lz4_decompress(chunk))
        elif bc == 0:
            blob.extend(chunk)
        else:
            raise ValueError(f"{path.name}: unsupported block compression {bc}")
    return nodes, bytes(blob)


def first_serialized_file(path: Path) -> tuple[bytes, int, list[str]]:
    """Returns (serialized_file_bytes, data_offset, node_names) for a bundle."""
    nodes, blob = unpack_unityfs(path)
    off, sz, _nf, _name = nodes[0]
    sf = blob[off : off + sz]
    ver = struct.unpack(">I", sf[8:12])[0]
    data_off = (
        struct.unpack(">Q", sf[32:40])[0]
        if ver >= 22
        else struct.unpack(">I", sf[12:16])[0]
    )
    return sf, data_off, [n[3] for n in nodes]


def load_manifest(mtga: Path) -> list[dict[str, Any]]:
    downloads = mtga / "MTGA_Data/Downloads"
    manifests = sorted(
        p
        for p in glob.glob(str(downloads / "Manifest_*.mtga"))
        if "Audio" not in os.path.basename(p)
        and "Localization" not in os.path.basename(p)
    )
    if not manifests:
        raise FileNotFoundError(f"No primary Manifest_*.mtga found in {downloads}")
    with open(manifests[0], "r", encoding="utf-8") as f:
        return json.load(f).get("Assets", [])


def find_bundle(mtga: Path, query: str) -> Path:
    ab_dir = mtga / "MTGA_Data/Downloads/AssetBundle"
    direct = ab_dir / query
    if direct.is_file():
        return direct
    matches = sorted(p for p in ab_dir.iterdir() if query.lower() in p.name.lower())
    if not matches:
        raise FileNotFoundError(f"No bundle matching {query!r} in {ab_dir}")
    if len(matches) > 1:
        print(
            f"# {len(matches)} bundles match {query!r}; using {matches[0].name}",
            file=sys.stderr,
        )
    return matches[0]


def cmd_manifest(mtga: Path, args: argparse.Namespace) -> None:
    pattern = re.compile(args.pattern, re.IGNORECASE)
    shown = 0
    for entry in load_manifest(mtga):
        name = entry.get("Name", "")
        if not args.include_card_art and "_cardart_" in name.lower():
            continue
        indexed = entry.get("IndexedAssets", [])
        if pattern.search(name) or any(pattern.search(a) for a in indexed):
            print(f"{name} ({entry.get('Length', 0):,} B)")
            for asset in indexed[: args.max_assets]:
                print(f"  {asset}")
            shown += 1
            if shown >= args.limit:
                break
    print(f"# showed {shown} matching bundle(s)")


def cmd_prefab(mtga: Path, args: argparse.Namespace) -> None:
    bundle = find_bundle(mtga, args.bundle)
    sf, data_off, node_names = first_serialized_file(bundle)
    print(f"Bundle: {bundle.name}")
    print(f"Nodes:  {node_names}")
    cabs = sorted(
        set(
            m.group(0).decode("ascii")
            for m in re.finditer(rb"CAB-[0-9a-f]{32}", sf[:data_off])
        )
    )
    print(f"External CABs ({len(cabs)}): {cabs}")
    strings: list[str] = []
    seen: set[str] = set()
    for m in re.finditer(rb"[A-Za-z0-9_/\-.]{4,90}", sf[data_off:]):
        s = m.group(0).decode("ascii", "ignore")
        if s not in seen and re.search(r"[A-Za-z]{3,}", s):
            seen.add(s)
            strings.append(s)
    print(f"Data strings ({len(strings)}, showing up to {args.limit}):")
    for s in strings[: args.limit]:
        print(f"  {s}")


def cmd_cabs(mtga: Path, args: argparse.Namespace) -> None:
    bundle = find_bundle(mtga, args.bundle)
    sf, data_off, _ = first_serialized_file(bundle)
    wanted = set(
        m.group(0).decode("ascii")
        for m in re.finditer(rb"CAB-[0-9a-f]{32}", sf[:data_off])
    )
    print(f"Resolving {len(wanted)} CAB dependencies of {bundle.name}...")
    ab_dir = mtga / "MTGA_Data/Downloads/AssetBundle"
    for entry in load_manifest(mtga):
        name = entry.get("Name", "")
        if "_CardArt_" in name:
            continue
        p = ab_dir / name
        if not p.is_file():
            continue
        try:
            nodes, _, _, _ = read_unityfs_header(p)
        except Exception:
            continue
        for _off, _sz, _nf, nname in nodes:
            if nname in wanted:
                print(f"  {nname} -> {name}")
                for idx in entry.get("IndexedAssets", [])[:8]:
                    print(f"    {idx}")


def cmd_materials(mtga: Path, args: argparse.Namespace) -> None:
    bundle = find_bundle(mtga, args.bundle)
    sf, data_off, _ = first_serialized_file(bundle)
    dp = sf[data_off:]
    print(f"Materials in {bundle.name}:")
    # Locate Unity Material blocks by scanning for m_ValidKeywords / RenderType or _MainTex/_Texture
    anchors = [
        m.start() for m in re.finditer(rb"(?:_TintColor|_MainTex|_Texture|_Color2)", dp)
    ]
    visited: list[int] = []
    for anchor in anchors:
        if any(abs(anchor - v) < 2500 for v in visited):
            continue
        visited.append(anchor)
        start = max(0, anchor - 1200)
        window = dp[start : anchor + 3200]
        # Guess nearby material name and active keywords
        kws = sorted(
            set(
                m.group(0).decode("ascii")
                for m in re.finditer(rb"[A-Z_][A-Z0-9_]{5,40}_(?:ON|OFF)", window)
            )
        )
        names = [
            m.group(0).decode("ascii")
            for m in re.finditer(rb"(?:mat_|M_|NPE|Mat_)[A-Za-z0-9_]{3,50}", window)
        ]
        label = names[0] if names else f"material@{start}"
        print(f"\n--- {label} ---")
        if kws:
            print(f"  keywords: {', '.join(kws)}")
        for prop in MATERIAL_PROPS:
            ppos = window.find(prop)
            if ppos < 4:
                continue
            slen = struct.unpack("<I", window[ppos - 4 : ppos])[0]
            if slen != len(prop):
                continue
            after = ppos + ((slen + 3) & ~3)
            if b"Color" in prop and after + 16 <= len(window):
                vals = struct.unpack("<4f", window[after : after + 16])
                print(
                    f"  color {prop.decode():22s} = "
                    f"({vals[0]:.3f}, {vals[1]:.3f}, {vals[2]:.3f}, {vals[3]:.3f})"
                )
            elif after + 4 <= len(window):
                val = struct.unpack("<f", window[after : after + 4])[0]
                print(f"  float {prop.decode():22s} = {val:.4f}")
        for tex in MATERIAL_TEX_PROPS:
            ppos = window.find(tex)
            if ppos < 4:
                continue
            slen = struct.unpack("<I", window[ppos - 4 : ppos])[0]
            if slen != len(tex):
                continue
            after = ppos + ((slen + 3) & ~3)
            if after + 28 <= len(window):
                fid, pid, sx, sy, ox, oy = struct.unpack(
                    "<iq4f", window[after : after + 28]
                )
                print(
                    f"  tex   {tex.decode():22s} fileID={fid} pathID={pid} "
                    f"scale=({sx:.3f},{sy:.3f}) off=({ox:.3f},{oy:.3f})"
                )


def _read_vec_vec_u32(buf: bytes, pos: int) -> tuple[list[list[int]], int]:
    outer = struct.unpack("<I", buf[pos : pos + 4])[0]
    pos += 4
    res: list[list[int]] = []
    for _ in range(outer):
        inner = struct.unpack("<I", buf[pos : pos + 4])[0]
        pos += 4
        res.append(list(struct.unpack(f"<{inner}I", buf[pos : pos + 4 * inner])))
        pos += 4 * inner
    return res, pos


def _extract_windows_glsl(
    sf: bytes, data_off: int, bundle_name: str, args: argparse.Namespace
) -> bool:
    """Extracts plain-text GLSL (platform 15 / glcore) from a Windows Shader bundle."""
    # Unity Shader binary layout ends with:
    #   vector<u32> platforms; vector<vector<u32>> offsets, compressedLengths,
    #   decompressedLengths; vector<u8> compressedBlob;
    hdr = sf.find(struct.pack("<III", 2, 4, 15), data_off)
    if hdr == -1:
        return False
    pos = hdr + 12
    offs, pos = _read_vec_vec_u32(sf, pos)
    clens, pos = _read_vec_vec_u32(sf, pos)
    _dlens, pos = _read_vec_vec_u32(sf, pos)
    blob_len = struct.unpack("<I", sf[pos : pos + 4])[0]
    blob_start = pos + 4
    if blob_start + blob_len > len(sf) or len(offs) < 2:
        return False
    hits = 0
    seen: set[int] = set()
    for off, clen in zip(offs[1], clens[1]):
        dec = lz4_decompress(sf[blob_start + off : blob_start + off + clen])
        if b"#version" not in dec:
            continue
        txt = dec.decode("utf-8", "replace")
        for part in txt.split("#version ")[1:]:
            end = part.find("\x00")
            if end != -1:
                part = part[:end]
            is_frag = "SV_Target" in part or "gl_FragColor" in part
            is_vert = "gl_Position" in part
            if args.stage == "fragment" and not is_frag:
                continue
            if args.stage == "vertex" and not is_vert:
                continue
            mpos = part.find("void main()")
            body = part[mpos:] if mpos != -1 else part
            if all(tok in body for tok in args.match):
                sig = hash(body[:400])
                if sig not in seen:
                    seen.add(sig)
                    hits += 1
                    print(
                        f"\n=== {bundle_name} {args.stage} GLSL #{hits} ({len(body)} chars) ==="
                    )
                    print(body[: args.max_chars])
                    if hits >= args.limit:
                        return True
    if hits == 0:
        print(f"# no {args.stage} GLSL programs matched {args.match} in {bundle_name}")
    return True


def cmd_shader(mtga: Path, args: argparse.Namespace) -> None:
    bundle = find_bundle(mtga, args.bundle)
    sf, data_off, _ = first_serialized_file(bundle)
    if b"metal_stdlib" not in sf and _extract_windows_glsl(
        sf, data_off, bundle.name, args
    ):
        return
    stage_token = f"{args.stage} "
    pos = 0
    hits = 0
    seen: set[int] = set()
    while hits < args.limit:
        idx = sf.find(b"metal_stdlib", pos)
        if idx == -1:
            break
        pos = idx + 12
        for back in range(16, 240):
            start = idx - back
            if start < 0:
                continue
            dec = bytearray()
            i = start
            n = min(len(sf), start + 32768)
            while i < n and len(dec) < 60000:
                token = sf[i]
                i += 1
                ll = token >> 4
                if ll == 15:
                    while i < n:
                        b = sf[i]
                        i += 1
                        ll += b
                        if b != 255:
                            break
                if i + ll > n:
                    break
                dec.extend(sf[i : i + ll])
                i += ll
                if i + 2 > n:
                    break
                off2 = sf[i] | (sf[i + 1] << 8)
                i += 2
                if off2 == 0 or off2 > len(dec):
                    break
                ml = (token & 0x0F) + 4
                if (token & 0x0F) == 15:
                    while i < n:
                        b = sf[i]
                        i += 1
                        ml += b
                        if b != 255:
                            break
                st = len(dec) - off2
                for k in range(ml):
                    dec.append(dec[st + k])
            if stage_token.encode() in dec:
                txt = dec.decode("utf-8", "replace")
                for part in txt.split("#include <metal_stdlib>")[1:]:
                    end = part.find("\x00")
                    if end != -1:
                        part = part[:end]
                    mpos = part.find(stage_token)
                    if mpos == -1:
                        continue
                    body = part[mpos:]
                    if all(tok in body for tok in args.match):
                        sig = hash(body[:400])
                        if sig not in seen:
                            seen.add(sig)
                            hits += 1
                            print(
                                f"\n=== {bundle.name} {args.stage} #{hits} ({len(body)} chars) ==="
                            )
                            print(body[: args.max_chars])
                            if hits >= args.limit:
                                break
                break
    if hits == 0:
        print(f"# no {args.stage} MSL programs matched {args.match} in {bundle.name}")


def _is_mono_install(mtga: Path) -> bool:
    il2cpp_meta = (
        mtga
        / "MTGA.app/Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat"
    )
    return not il2cpp_meta.is_file() and (mtga / "MTGA_Data/Managed/Core.dll").is_file()


def cmd_symbols(mtga: Path, args: argparse.Namespace) -> None:
    if _is_mono_install(mtga):
        cmd_mono_symbols(mtga, args)
        return
    meta = (
        mtga
        / "MTGA.app/Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat"
    )
    data = meta.read_bytes()
    magic, version = struct.unpack("<II", data[:8])
    print(
        f"# global-metadata.dat magic=0x{magic:08x} version={version} ({len(data):,} B)"
    )
    pattern = re.compile(args.pattern, re.IGNORECASE)
    matches = sorted(
        set(
            s
            for m in re.finditer(rb"[A-Za-z_][A-Za-z0-9_]{3,90}", data)
            if pattern.search(s := m.group(0).decode("ascii"))
        )
    )
    print(f"# {len(matches)} matches (showing up to {args.limit}):")
    for s in matches[: args.limit]:
        print(f"  {s}")


def cmd_types(mtga: Path, args: argparse.Namespace) -> None:
    if _is_mono_install(mtga):
        cmd_mono_types(mtga, args)
        return
    cmd_il2cpp_types(mtga, args)


def cmd_disasm(mtga: Path, args: argparse.Namespace) -> None:
    if _is_mono_install(mtga):
        cmd_mono_disasm(mtga, args)
        return
    cmd_il2cpp_disasm(mtga, args)


def cmd_alt(mtga: Path, args: argparse.Namespace) -> None:
    alt_dir = mtga / "MTGA_Data/Downloads/ALT"
    query = args.file.lower()
    exact = sorted(
        p
        for p in alt_dir.iterdir()
        if p.name.lower() == query or p.name.lower().startswith(query + "_")
    )
    matches = exact or sorted(p for p in alt_dir.iterdir() if query in p.name.lower())
    if not matches:
        raise FileNotFoundError(f"No ALT_*.mtga matching {args.file!r} in {alt_dir}")
    path = matches[0]
    data = json.loads(path.read_text(encoding="utf-8"))
    print(f"# {path.name} ({len(data)} trees)")
    if not args.pattern:
        for key, val in sorted(data.items()):
            nodes = val.get("Nodes", []) if isinstance(val, dict) else []
            print(f"  {key} ({len(nodes)} nodes)")
        return
    pattern = re.compile(args.pattern, re.IGNORECASE)
    shown = 0
    for tree_name, tree in sorted(data.items()):
        if not isinstance(tree, dict) or "Nodes" not in tree:
            continue
        tree_hits = bool(pattern.search(tree_name))
        nodes = {n["NodeId"]: n for n in tree.get("Nodes", []) if "NodeId" in n}
        parents: dict[str, list[tuple[str, str]]] = {}
        for conn in tree.get("Connections", []):
            parent = conn.get("Parent", "")
            child = conn.get("Child")
            if isinstance(child, str):
                parents.setdefault(child, []).append((parent, ""))
            elif isinstance(child, list):
                for dst in child:
                    if isinstance(dst, str):
                        parents.setdefault(dst, []).append((parent, ""))
            elif isinstance(child, dict):
                for edge_key, dst in child.items():
                    dsts = dst if isinstance(dst, list) else [dst]
                    for item in dsts:
                        if isinstance(item, str):
                            parents.setdefault(item, []).append((parent, str(edge_key)))
        for nid, node in nodes.items():
            summary = json.dumps({k: v for k, v in node.items() if k != "NodeId"})
            if not tree_hits and not pattern.search(summary):
                continue
            chain: list[tuple[str, str]] = []
            cur: str | None = nid
            edge = ""
            seen_ids: set[str] = set()
            while cur and cur not in seen_ids and len(chain) < 16:
                seen_ids.add(cur)
                n = nodes.get(cur, {})
                brief = {
                    k: v
                    for k, v in n.items()
                    if k not in ("NodeId", "Position", "Name")
                    and v not in ("", [], None)
                }
                chain.append((edge, json.dumps(brief, separators=(",", ":"))))
                plist = parents.get(cur)
                if plist:
                    cur, edge = plist[0]
                else:
                    cur, edge = None, ""
            print(f"\n[{tree_name}]")
            # `edge` on chain[i] is the edge leaving chain[i+1] into chain[i].
            steps = list(reversed(chain))
            for depth, (_out_edge, step) in enumerate(steps):
                in_edge = steps[depth - 1][0] if depth > 0 else ""
                tag = f"[{in_edge}] " if in_edge else ""
                print(f"  {'  ' * depth}-> {tag}{step}")
            shown += 1
            if shown >= args.limit:
                print(f"\n# reached limit {args.limit}")
                return
    print(f"\n# showed {shown} matching ALT path(s)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mtga", type=Path, default=DEFAULT_MTGA, help="MTGA install directory"
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_man = sub.add_parser(
        "manifest", help="Search Manifest_*.mtga for bundles and IndexedAssets"
    )
    p_man.add_argument("pattern", help="Regex pattern to match bundle or asset path")
    p_man.add_argument("--limit", type=int, default=30)
    p_man.add_argument("--max-assets", type=int, default=15)
    p_man.add_argument("--include-card-art", action="store_true")

    p_alt = sub.add_parser(
        "alt", help="Inspect Asset Lookup Trees (ALT_*.mtga) and trace condition chains"
    )
    p_alt.add_argument("file", help="ALT file prefix (e.g. ALT_Card, ALT_ZoneTransfer)")
    p_alt.add_argument("pattern", nargs="?", default="", help="Optional regex to trace")
    p_alt.add_argument("--limit", type=int, default=20)

    p_pre = sub.add_parser(
        "prefab", help="Inspect nodes, external CABs, and strings in a bundle"
    )
    p_pre.add_argument("bundle", help="Bundle filename or substring")
    p_pre.add_argument("--limit", type=int, default=80)

    p_cab = sub.add_parser(
        "cabs", help="Resolve external CAB-* dependencies of a bundle"
    )
    p_cab.add_argument("bundle", help="Bundle filename or substring")

    p_mat = sub.add_parser(
        "materials", help="Dump serialized Material properties in a bundle"
    )
    p_mat.add_argument("bundle", help="Bundle filename or substring")

    p_shd = sub.add_parser(
        "shader",
        help="Extract plain-text shader bodies (MSL on macOS, GLSL on Windows) from a Shader_* bundle",
    )
    p_shd.add_argument("bundle", help="Shader bundle filename or substring")
    p_shd.add_argument("--stage", choices=["fragment", "vertex"], default="fragment")
    p_shd.add_argument(
        "--match", action="append", default=[], help="Substring required in shader body"
    )
    p_shd.add_argument("--limit", type=int, default=2)
    p_shd.add_argument("--max-chars", type=int, default=4500)

    p_sym = sub.add_parser(
        "symbols",
        help="Search unobfuscated C# identifiers (global-metadata.dat on macOS, Managed/*.dll on Windows)",
    )
    p_sym.add_argument("pattern", help="Regex pattern")
    p_sym.add_argument("--limit", type=int, default=60)

    p_typ = sub.add_parser(
        "types",
        help="Dump fields and method tokens for C# types (global-metadata.dat or Managed/<assembly>)",
    )
    p_typ.add_argument("pattern", help="Regex matching Namespace.TypeName")
    p_typ.add_argument("--assembly", default="Core.dll")
    p_typ.add_argument("--limit", type=int, default=10)
    p_typ.add_argument("--max-members", type=int, default=40)

    p_dis = sub.add_parser(
        "disasm",
        help="Disassemble C# method(s) (ARM64 from GameAssembly.dylib on macOS, CIL from Managed/<assembly> on Windows)",
    )
    p_dis.add_argument("type_pattern", help="Regex matching Namespace.TypeName")
    p_dis.add_argument("method_pattern", help="Regex matching method name")
    p_dis.add_argument("--assembly", default="Core.dll")
    p_dis.add_argument("--limit", type=int, default=5)
    p_dis.add_argument("--max-bytes", type=int, default=0x500)
    p_dis.add_argument("--max-lines", type=int, default=85)

    args = parser.parse_args()
    handlers = {
        "manifest": cmd_manifest,
        "alt": cmd_alt,
        "prefab": cmd_prefab,
        "cabs": cmd_cabs,
        "materials": cmd_materials,
        "shader": cmd_shader,
        "symbols": cmd_symbols,
        "types": cmd_types,
        "disasm": cmd_disasm,
    }
    handlers[args.cmd](args.mtga, args)


if __name__ == "__main__":
    main()
