// The cover and the song's identity: the picture, title and artist from
// the mp3's ID3v2 tag, the app's four-colour palette
// (gpui/crates/visuals/src/cover.rs: quadrant averages, saturation lifted
// 1.4) and how much colour the tone mapping keeps (renderer.rs
// colour_kept), and the seed from a video id.

// Test palettes for the taste checks: a vivid, a muted and a
// near-monochrome cover. Also used when the mp3 has no picture.
export const TEST_PALETTES = {
  vivid: [[0.86, 0.28, 0.22], [0.98, 0.62, 0.24], [0.22, 0.36, 0.78], [0.42, 0.18, 0.52]],
  muted: [[0.48, 0.55, 0.47], [0.7, 0.66, 0.55], [0.35, 0.42, 0.46], [0.27, 0.3, 0.28]],
  mono: [[0.55, 0.5, 0.45], [0.72, 0.69, 0.64], [0.32, 0.3, 0.28], [0.18, 0.17, 0.16]],
};

// Title, artist and picture (a Blob or null) from an mp3's ID3v2 tag.
export function readTag(buffer) {
  const bytes = new Uint8Array(buffer);
  const out = { title: null, artist: null, picture: null };
  if (bytes.length < 10 || bytes[0] !== 0x49 || bytes[1] !== 0x44 || bytes[2] !== 0x33) return out;
  const major = bytes[3];
  const flags = bytes[5];
  const end = Math.min(10 + syncsafe(bytes, 6), bytes.length);
  let at = 10;
  if (flags & 0x40) at += major === 4 ? syncsafe(bytes, at) : 4 + be32(bytes, at);
  const idLen = major === 2 ? 3 : 4;
  const headLen = major === 2 ? 6 : 10;
  while (at + headLen <= end) {
    const id = String.fromCharCode(...bytes.subarray(at, at + idLen));
    if (!/^[A-Z0-9]+$/.test(id)) break;
    const size = major === 2 ? (bytes[at + 3] << 16) | (bytes[at + 4] << 8) | bytes[at + 5]
      : major === 4 ? syncsafe(bytes, at + 4) : be32(bytes, at + 4);
    const body = bytes.subarray(at + headLen, at + headLen + size);
    if (id === "TIT2" || id === "TT2") out.title = text(body);
    if (id === "TPE1" || id === "TP1") out.artist = text(body);
    if ((id === "APIC" || id === "PIC") && !out.picture) out.picture = picture(body, major);
    at += headLen + size;
  }
  return out;
}

function syncsafe(b, i) {
  return (b[i] << 21) | (b[i + 1] << 14) | (b[i + 2] << 7) | b[i + 3];
}

function be32(b, i) {
  return ((b[i] << 24) | (b[i + 1] << 16) | (b[i + 2] << 8) | b[i + 3]) >>> 0;
}

function decoder(encoding) {
  return new TextDecoder(["latin1", "utf-16", "utf-16be", "utf-8"][encoding] ?? "latin1");
}

function text(body) {
  return decoder(body[0]).decode(body.subarray(1)).replace(/\0+$/, "").split("\0")[0].trim() || null;
}

function picture(body, major) {
  const encoding = body[0];
  let at = 1;
  let mime = "image/jpeg";
  if (major === 2) {
    if (String.fromCharCode(...body.subarray(1, 4)) === "PNG") mime = "image/png";
    at = 4;
  } else {
    const zero = body.indexOf(0, at);
    mime = new TextDecoder("latin1").decode(body.subarray(at, zero)) || mime;
    at = zero + 1;
  }
  at += 1; // picture type
  // The description ends in one zero, or two for UTF-16.
  if (encoding === 1 || encoding === 2) {
    while (at + 1 < body.length && (body[at] || body[at + 1])) at += 2;
    at += 2;
  } else {
    while (at < body.length && body[at]) at += 1;
    at += 1;
  }
  if (!mime.includes("/")) mime = `image/${mime.toLowerCase()}`;
  return new Blob([body.subarray(at)], { type: mime });
}

// The app's palette from a decoded image (anything drawImage takes).
export function palette(image) {
  const n = 48;
  const canvas = new OffscreenCanvas(n, n);
  const ctx = canvas.getContext("2d");
  ctx.drawImage(image, 0, 0, n, n);
  const rgba = ctx.getImageData(0, 0, n, n).data;
  const half = n / 2;
  return [0, 1, 2, 3].map((q) => {
    const qx = (q % 2) * half;
    const qy = Math.floor(q / 2) * half;
    const sum = [0, 0, 0];
    for (let y = qy; y < qy + half; y++) {
      for (let x = qx; x < qx + half; x++) {
        const i = (y * n + x) * 4;
        for (let c = 0; c < 3; c++) sum[c] += rgba[i + c] / 255;
      }
    }
    const avg = sum.map((s) => s / (half * half));
    const grey = (avg[0] + avg[1] + avg[2]) / 3;
    return avg.map((c) => Math.min(Math.max(grey + (c - grey) * 1.4, 0), 1));
  });
}

// renderer.rs colour_kept: all of the colour for a colourful cover, a
// quarter for a sepia or black-and-white one.
export function colourKept(pal) {
  let chroma = 0;
  for (const c of pal) {
    const lab = oklab(c.map(toLinear));
    chroma = Math.max(chroma, Math.hypot(lab[1], lab[2]));
  }
  return 0.25 + 0.75 * Math.min(Math.max((chroma - 0.05) / 0.07, 0), 1);
}

function toLinear(v) {
  return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
}

function oklab([r, g, b]) {
  const l = Math.cbrt(Math.max(0.41222146 * r + 0.53633255 * g + 0.051445995 * b, 0));
  const m = Math.cbrt(Math.max(0.2119035 * r + 0.6806995 * g + 0.10739696 * b, 0));
  const s = Math.cbrt(Math.max(0.08830246 * r + 0.28171885 * g + 0.6299787 * b, 0));
  return [
    0.21045426 * l + 0.7936178 * m - 0.004072047 * s,
    1.9779985 * l - 2.4285922 * m + 0.4505937 * s,
    0.025904037 * l + 0.78277177 * m - 0.80867577 * s,
  ];
}

// Four values 0..1 from a video id (FNV-1a, then PCG steps), so the same
// id always gives the same world.
export function seed(id) {
  let h = 0x811c9dc5;
  for (const ch of id) {
    h ^= ch.codePointAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  const out = [];
  for (let i = 0; i < 4; i++) {
    h = (Math.imul(h, 747796405) + 2891336453) >>> 0;
    let w = Math.imul((h >>> ((h >>> 28) + 4)) ^ h, 277803737) >>> 0;
    w = ((w >>> 22) ^ w) >>> 0;
    out.push(w / 4294967295);
  }
  return out;
}
