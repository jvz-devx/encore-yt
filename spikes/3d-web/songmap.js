// The song as a map for the world scene: the whole decoded track analysed
// once, four rows a second, then baked into a small height texture the
// shader flies over (one row per quarter second of the song, 64 lanes
// across the valley).
//
// Per row: the 12 pitch classes (chroma), loudness, brightness (spectral
// centroid) and onset strength. Over the song: its key and mode
// (Krumhansl-Kessler profiles) and a character that picks the biome.
//
// Lanes: the pitch classes stand across the valley in circle-of-fifths
// order around the key. The tonic is the pair of walls either side of the
// flight path; the dominant side goes right, the subdominant side left,
// and the further a note is from the key, the further out its range. So
// the song's harmony is the layout of the land, its dynamics the height
// (quiet passages sink under the water), its timbre the roughness.

import { fft } from "./analysis.js";

export const ROWS_PER_SECOND = 4;
export const LANES = 64;
const WINDOW = 8192;
const NOTE_LOW = 65; // C2
const NOTE_HIGH = 2100; // C7
const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const MAJOR = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const BIOMES = ["hills", "alpine", "mesa", "islands"];

// `samples`: mono Float32Array at `rate`. Returns the baked texture data
// (rgba per lane and row: r height, g brightness, b energy, a onsets, all
// 0..1) and what the analysis found.
export function songMap(samples, rate) {
  const duration = samples.length / rate;
  const rows = Math.max(2, Math.min(8192, Math.ceil(duration * ROWS_PER_SECOND)));
  const window = Float32Array.from({ length: WINDOW }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / WINDOW));
  const re = new Float32Array(WINDOW);
  const im = new Float32Array(WINDOW);
  const half = WINDOW / 2;
  // Which pitch class each bin belongs to, or -1.
  const pcOf = new Int8Array(half);
  for (let k = 0; k < half; k++) {
    const f = (k * rate) / WINDOW;
    pcOf[k] = f < NOTE_LOW || f > NOTE_HIGH ? -1 : ((Math.round(12 * Math.log2(f / 440) + 69) % 12) + 12) % 12;
  }
  const chroma = Array.from({ length: rows }, () => new Float32Array(12));
  const db = new Float32Array(rows);
  const centroid = new Float32Array(rows);
  const flux = new Float32Array(rows);
  let prev = new Float32Array(half);
  let mag = new Float32Array(half);

  for (let r = 0; r < rows; r++) {
    const centre = Math.round((r / ROWS_PER_SECOND) * rate);
    let sum = 0;
    for (let i = 0; i < WINDOW; i++) {
      const at = centre - half + i;
      const v = at >= 0 && at < samples.length ? samples[at] : 0;
      sum += v * v;
      re[i] = v * window[i];
      im[i] = 0;
    }
    db[r] = 10 * Math.log10(sum / WINDOW + 1e-12);
    fft(re, im);
    let weighted = 0;
    let total = 0;
    let rise = 0;
    for (let k = 1; k < half; k++) {
      const m = Math.hypot(re[k], im[k]);
      mag[k] = m;
      const f = (k * rate) / WINDOW;
      if (f > 50 && f < 10000) {
        weighted += f * m;
        total += m;
      }
      rise += Math.max(0, m - prev[k]);
      if (pcOf[k] >= 0) chroma[r][pcOf[k]] += m;
    }
    centroid[r] = total > 0 ? weighted / total : 0;
    flux[r] = rise;
    [prev, mag] = [mag, prev];
    const top = Math.max(...chroma[r], 1e-9);
    for (let c = 0; c < 12; c++) chroma[r][c] /= top;
  }

  // The key: the whole song's chroma against the major and minor profiles
  // in every rotation, loud rows counting more.
  const loudness = normalise(db, 0.1, 0.95);
  const total = new Float32Array(12);
  chroma.forEach((c, r) => c.forEach((v, i) => (total[i] += v * loudness[r])));
  let best = { score: -Infinity, tonic: 0, major: true };
  for (let tonic = 0; tonic < 12; tonic++) {
    for (const [profile, major] of [[MAJOR, true], [MINOR, false]]) {
      const score = correlation(total, (i) => profile[(i - tonic + 12) % 12]);
      if (score > best.score) best = { score, tonic, major };
    }
  }

  // Per row, smoothed so ranges flow: notes over about 1.5 s, the energy
  // that raises and sinks the land over about 6 s.
  const brightness = smooth(normalise(centroid.map((c) => Math.log2(Math.max(c, 50))), 0.05, 0.95), 4);
  const onsets = smooth(normalise(flux, 0.2, 0.97), 2);
  const energy = normalise(smooth(loudness, 24), 0.0, 1.0);
  const notes = Array.from({ length: 12 }, (_, c) => smooth(Float32Array.from(chroma, (row, r) => {
    // Sharpen: the strongest notes stand, the wash of overtones falls.
    const v = row[c];
    return v * v * v * Math.min(1, loudness[r] * 1.6);
  }), 6));

  // Lane positions (-1..1) of each pitch class, by fifths from the tonic.
  const lanes = [];
  for (let c = 0; c < 12; c++) {
    const fifths = (((c - best.tonic) * 7) % 12 + 12) % 12;
    const s = fifths <= 6 ? fifths : fifths - 12;
    if (s === 0) lanes.push([c, -0.22], [c, 0.22]);
    else lanes.push([c, Math.sign(s) * (0.22 + 0.115 * Math.abs(s))]);
  }

  const data = new Float32Array(LANES * rows * 4);
  for (let r = 0; r < rows; r++) {
    const width = 0.055 + 0.035 * energy[r];
    for (let j = 0; j < LANES; j++) {
      const u = (j / (LANES - 1)) * 2 - 1;
      let h = 0;
      for (const [c, at] of lanes) {
        const d = (u - at) / width;
        h += notes[c][r] * Math.exp(-d * d);
      }
      // The ranges rise with the music's energy; the valley floor too, a
      // little, so loud passages are high country throughout.
      const height = Math.min(1, h * 0.8) * (0.25 + 0.75 * energy[r]) + 0.18 * energy[r];
      const o = (r * LANES + j) * 4;
      data[o] = height;
      data[o + 1] = brightness[r];
      data[o + 2] = energy[r];
      data[o + 3] = onsets[r];
    }
  }

  const meanCentroid = mean(centroid.filter((_, r) => loudness[r] > 0.3));
  const range = percentile(db, 0.95) - percentile(db, 0.1);
  const busy = mean(onsets);
  const biome = meanCentroid > 2200 ? 1 : meanCentroid < 1100 ? 2 : range > 20 || busy < 0.2 ? 3 : 0;
  return {
    rows,
    duration,
    data,
    key: `${NAMES[best.tonic]} ${best.major ? "major" : "minor"}`,
    // Major keys get day and golden hour, minor ones dusk.
    mood: best.major ? 0.75 : 0.25,
    biome,
    biomeName: BIOMES[biome],
    stats: { meanCentroid: Math.round(meanCentroid), dynamicRange: Math.round(range), busy: busy.toFixed(2) },
  };
}

function normalise(values, lowP, highP) {
  const lo = percentile(values, lowP);
  const hi = percentile(values, highP);
  return Float32Array.from(values, (v) => Math.min(1, Math.max(0, (v - lo) / Math.max(hi - lo, 1e-9))));
}

function percentile(values, p) {
  const sorted = Float32Array.from(values).sort();
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
}

function mean(values) {
  let s = 0;
  for (const v of values) s += v;
  return values.length ? s / values.length : 0;
}

// Exponential smoothing forwards then backwards (no lag), over about
// `span` rows.
function smooth(values, span) {
  const a = 1 / Math.max(span, 1);
  const out = Float32Array.from(values);
  for (let i = 1; i < out.length; i++) out[i] += (out[i - 1] - out[i]) * (1 - a);
  for (let i = out.length - 2; i >= 0; i--) out[i] += (out[i + 1] - out[i]) * (1 - a);
  return out;
}

function correlation(xs, profileAt) {
  const n = xs.length;
  let mx = 0;
  let my = 0;
  for (let i = 0; i < n; i++) {
    mx += xs[i];
    my += profileAt(i);
  }
  mx /= n;
  my /= n;
  let sxy = 0;
  let sxx = 0;
  let syy = 0;
  for (let i = 0; i < n; i++) {
    const dx = xs[i] - mx;
    const dy = profileAt(i) - my;
    sxy += dx * dy;
    sxx += dx * dx;
    syy += dy * dy;
  }
  return sxy / Math.sqrt(sxx * syy + 1e-12);
}

// A flat, quiet map for when no song is loaded.
export function emptyMap() {
  const rows = 2;
  const data = new Float32Array(LANES * rows * 4);
  for (let i = 0; i < LANES * rows; i++) data.set([0.25, 0.4, 0.4, 0.2], i * 4);
  return { rows, duration: 0, data, key: "no song", mood: 0.6, biome: 0, biomeName: "hills", stats: {} };
}

// The baked map as rgba16float texels (Uint16 half floats).
export function halfFloats(data) {
  const f32 = new Float32Array(1);
  const u32 = new Uint32Array(f32.buffer);
  return Uint16Array.from(data, (v) => {
    f32[0] = v;
    const x = u32[0];
    const sign = (x >>> 16) & 0x8000;
    const e = ((x >>> 23) & 0xff) - 112;
    if (e <= 0) return sign;
    if (e >= 31) return sign | 0x7c00;
    return sign | (e << 10) | ((x >>> 13) & 0x3ff);
  });
}
