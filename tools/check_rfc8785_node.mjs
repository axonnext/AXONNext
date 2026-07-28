// Independent V8/ECMAScript cross-check of RFC 8785 Appendix B, plus a
// line-oriented binary64 oracle used by check_rfc8785_differential.py.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
if (process.argv.includes("--stdin-bits")) {
  const source = fs.readFileSync(0, "utf8");
  const lines = source.split(/\r?\n/).filter((line) => line.length > 0);
  const rendered = lines.map((bits) => {
    if (!/^[0-9a-fA-F]{16}$/.test(bits)) {
      throw new Error(`invalid binary64 bits: ${bits}`);
    }
    const value = Buffer.from(bits, "hex").readDoubleBE(0);
    if (!Number.isFinite(value)) {
      throw new Error(`non-finite binary64 oracle input: ${bits}`);
    }
    return JSON.stringify(value);
  });
  process.stdout.write(rendered.join("\n") + (rendered.length ? "\n" : ""));
} else {
  const corpus = JSON.parse(fs.readFileSync(
    path.join(root, "conformance", "rfc8785_appendix_b.json"), "utf8"));

  for (const item of corpus.cases) {
    const bytes = Buffer.from(item.bits, "hex");
    const value = bytes.readDoubleBE(0);
    const actual = JSON.stringify(value);
    const expected = item.jcs === null ? "null" : item.jcs;
    if (actual !== expected) {
      throw new Error(`${item.bits}: V8 emitted ${actual}, expected ${expected}`);
    }
  }
  console.log(`V8 verified ${corpus.cases.length} RFC 8785 Appendix B rows`);
}
