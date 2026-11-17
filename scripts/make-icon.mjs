// Membuat ikon aplikasi 1024x1024 (PNG) tanpa dependensi eksternal.
// Desain: konsol komando — layar hijau fosfor dengan crosshair kuningan
// di atas latar hijau-hitam pekat. Jalankan: node scripts/make-icon.mjs <out.png>
import zlib from "node:zlib";
import fs from "node:fs";

const SIZE = 1024;

// --- kanvas RGBA ---
const px = new Uint8Array(SIZE * SIZE * 4);

function set(x, y, r, g, b, a = 255) {
  if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const i = (y * SIZE + x) * 4;
  const alpha = a / 255;
  px[i] = Math.round(px[i] * (1 - alpha) + r * alpha);
  px[i + 1] = Math.round(px[i + 1] * (1 - alpha) + g * alpha);
  px[i + 2] = Math.round(px[i + 2] * (1 - alpha) + b * alpha);
  px[i + 3] = Math.max(px[i + 3], Math.round(a));
}

// Latar: gradasi radial hijau-hitam
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const dx = (x - SIZE / 2) / (SIZE / 2);
    const dy = (y - SIZE / 2) / (SIZE / 2);
    const d = Math.min(1, Math.sqrt(dx * dx + dy * dy));
    const t = 1 - d * 0.55;
    const r = Math.round(12 * t + 6);
    const g = Math.round(27 * t + 10);
    const b = Math.round(22 * t + 8);
    set(x, y, r, g, b);
  }
}

const round = (v) => Math.round(v);

// --- layar monitor (rounded rect) ---
const sx0 = 172, sy0 = 252, sx1 = 852, sy1 = 700, radius = 48;
const screenFill = [18, 34, 26];
const screenEdge = [123, 227, 130];
for (let y = sy0; y <= sy1; y++) {
  for (let x = sx0; x <= sx1; x++) {
    // jarak sudut untuk rounded rect
    const cx = Math.max(sx0 + radius - x, 0, x - (sx1 - radius));
    const cy = Math.max(sy0 + radius - y, 0, y - (sy1 - radius));
    const corner = Math.sqrt(cx * cx + cy * cy);
    if (corner <= radius) {
      const edge = corner > radius - 10;
      const [er, eg, eb] = edge ? screenEdge : screenFill;
      set(x, y, er, eg, eb, edge ? 255 : 255);
    }
  }
}

// scanline di dalam layar
for (let y = sy0 + 30; y < sy1 - 10; y += 6) {
  for (let x = sx0 + 16; x < sx1 - 16; x++) set(x, y, 60, 110, 80, 40);
}

// --- crosshair kuningan di tengah layar ---
const brass = [227, 168, 87];
const cx = round((sx0 + sx1) / 2);
const cy = round((sy0 + sy1) / 2);
const arm = 150;
for (let i = -arm; i <= arm; i++) {
  // garis horizontal & vertikal (gap di tengah)
  if (Math.abs(i) > 42) {
    for (let t = -5; t <= 5; t++) {
      set(cx + i, cy + t, ...brass);
      set(cx + t, cy + i, ...brass);
    }
  }
}
// lingkaran + titik pusat
for (let a = 0; a < 360; a += 0.25) {
  const rad = (a * Math.PI) / 180;
  for (let rr = 86; rr <= 92; rr++) {
    set(round(cx + Math.cos(rad) * rr), round(cy + Math.sin(rad) * rr), ...brass);
  }
}
for (let yy = -8; yy <= 8; yy++)
  for (let xx = -8; xx <= 8; xx++)
    if (xx * xx + yy * yy <= 64) set(cx + xx, cy + yy, ...brass);

// --- kaki monitor ---
for (let y = sy1; y < sy1 + 58; y++) {
  for (let x = cx - 12; x <= cx + 12; x++) set(x, y, ...screenEdge);
}
for (let x = cx - 130; x <= cx + 130; x++) {
  for (let y = sy1 + 58; y < sy1 + 92; y++) {
    const edge = x <= cx - 128 || x >= cx + 128 || y >= sy1 + 90;
    set(x, y, ...(edge ? brass : screenEdge));
  }
}

// --- gelombang sinyal di pojok kanan atas layar ---
for (let i = 0; i < 220; i++) {
  const x = sx0 + 60 + i;
  const y = round(sy0 + 80 - Math.sin(i / 26) * 26 * Math.exp(-((i - 110) ** 2) / 9000) * 3);
  set(x, y, ...brass);
  set(x, y + 1, ...brass);
}

// --- penulis PNG ---
function crc32(buf) {
  let c, table = [];
  for (let n = 0; n < 256; n++) {
    c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  let crc = 0xffffffff;
  for (const b of buf) crc = table[(crc ^ b) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0; // filter none
  Buffer.from(px.buffer, y * SIZE * 4, SIZE * 4).copy(raw, y * (SIZE * 4 + 1) + 1);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

const out = process.argv[2] ?? "app-icon.png";
fs.writeFileSync(out, png);
console.log(`ikon ditulis: ${out} (${png.length} bytes)`);
