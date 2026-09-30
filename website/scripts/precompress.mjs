// Write <file>.br and <file>.gz next to each file given, then verify that both
// decompress back to the source byte for byte. The server only serves a
// sidecar that is newer than its source, so rerun this after the file changes.
import { readFileSync, writeFileSync } from "node:fs";
import { brotliCompressSync, brotliDecompressSync, gzipSync, gunzipSync, constants } from "node:zlib";

for (const file of process.argv.slice(2)) {
  const src = readFileSync(file);
  const br = brotliCompressSync(src, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: src.length } });
  const gz = gzipSync(src, { level: 9 });
  if (!brotliDecompressSync(br).equals(src) || !gunzipSync(gz).equals(src)) {
    console.error("precompress: round trip failed for " + file);
    process.exit(1);
  }
  writeFileSync(file + ".br", br);
  writeFileSync(file + ".gz", gz);
  console.log(file + ": " + src.length + " -> br " + br.length + ", gz " + gz.length);
}
