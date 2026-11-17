// Reads the family and version from the `name` table of each font file it is given, TrueType
// or WOFF2, and prints one JSON object of file to {family, version}, or to {error}.
// `just b3d match snapshot` (fonts.py) uses it to check that Beryl 3D's bundled fonts are the
// faces the web interface draws with. It is Node, not Python, because a WOFF2 file's tables are
// compressed with Brotli, which Node's zlib has and Python's standard library lacks.
//
// Usage: node fonts.mjs <font file>...
import { readFileSync } from "node:fs";
import { brotliDecompressSync } from "node:zlib";

/** The tags a WOFF2 table directory names by index (the WOFF2 specification, 5.1). */
const KNOWN = (
    "cmap head hhea hmtx maxp name OS/2 post cvt  fpgm glyf loca prep CFF  VORG EBDT EBLC " +
    "gasp hdmx kern LTSH PCLT VDMX vhea vmtx BASE GDEF GPOS GSUB EBSC JSTF MATH CBDT CBLC " +
    "COLR CPAL SVG  sbix acnt avar bdat bloc bsln cvar fdsc feat fmtx fvar gvar hsty just " +
    "lcar mort morx opbd prop trak Zapf Silf Glat Gloc Feat Sill"
)
    .match(/.{4}\s?/g)
    .map((tag) => tag.slice(0, 4));

/** The `name` table of a TrueType or OpenType file. */
function sfntName(data) {
    const tables = data.readUInt16BE(4);
    for (let i = 0; i < tables; i++) {
        const at = 12 + i * 16;
        if (data.toString("latin1", at, at + 4) === "name") {
            const offset = data.readUInt32BE(at + 8);
            return data.subarray(offset, offset + data.readUInt32BE(at + 12));
        }
    }
    throw new Error("no name table");
}

/** The `name` table of a WOFF2 file, from its Brotli stream of every table end to end. */
function woff2Name(data) {
    const tables = data.readUInt16BE(12);
    const compressed = data.readUInt32BE(20);
    let at = 48;
    const base128 = () => {
        let value = 0;
        for (let i = 0; i < 5; i++) {
            const byte = data[at++];
            value = value * 128 + (byte & 0x7f);
            if (!(byte & 0x80)) return value;
        }
        throw new Error("a UIntBase128 longer than five bytes");
    };
    const directory = [];
    for (let i = 0; i < tables; i++) {
        const flags = data[at++];
        let tag = KNOWN[flags & 0x3f];
        if ((flags & 0x3f) === 63) {
            tag = data.toString("latin1", at, at + 4);
            at += 4;
        }
        const length = base128();
        const version = flags >> 6;
        // glyf and loca are transformed at version 0; every other table at any other version.
        const transformed =
            tag === "glyf" || tag === "loca" ? version === 0 : version !== 0;
        directory.push({ tag, length: transformed ? base128() : length });
    }
    const stream = brotliDecompressSync(data.subarray(at, at + compressed));
    let offset = 0;
    for (const { tag, length } of directory) {
        if (tag === "name") return stream.subarray(offset, offset + length);
        offset += length;
    }
    throw new Error("no name table");
}

/** Name `id` of a `name` table: Windows English first, then any Windows, then Macintosh. */
function nameOf(table, id) {
    const count = table.readUInt16BE(2);
    const strings = table.readUInt16BE(4);
    const records = [];
    for (let i = 0; i < count; i++) {
        const at = 6 + i * 12;
        records.push({
            platform: table.readUInt16BE(at),
            language: table.readUInt16BE(at + 4),
            id: table.readUInt16BE(at + 6),
            length: table.readUInt16BE(at + 8),
            offset: table.readUInt16BE(at + 10),
        });
    }
    const rank = (r) =>
        r.platform === 3 && r.language === 0x409 ? 0 : r.platform === 3 ? 1 : 2;
    const found = records.filter((r) => r.id === id).sort((a, b) => rank(a) - rank(b))[0];
    if (!found) return null;
    const bytes = table.subarray(strings + found.offset, strings + found.offset + found.length);
    if (found.platform === 3 || found.platform === 0) {
        return Buffer.from(bytes).swap16().toString("utf16le");
    }
    return bytes.toString("latin1");
}

const out = {};
for (const file of process.argv.slice(2)) {
    try {
        const data = readFileSync(file);
        const table =
            data.toString("latin1", 0, 4) === "wOF2" ? woff2Name(data) : sfntName(data);
        out[file] = {
            // The typographic family (16) where the font has one, else the family (1).
            family: nameOf(table, 16) ?? nameOf(table, 1),
            version: nameOf(table, 5),
        };
    } catch (e) {
        out[file] = { error: String(e.message ?? e) };
    }
}
console.log(JSON.stringify(out));
