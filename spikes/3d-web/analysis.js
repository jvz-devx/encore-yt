// The app's audio analysis (gpui/crates/visuals/src/spectrum.rs) on Web
// Audio: 60 hops a second, a 4096-point FFT of the newest mono samples
// folded into 32 log-spaced bands from 50 Hz to 16 kHz, automatic gain,
// fast attack and slow release, then bass, kick and level. On top of that
// the slow envelopes the 3D scenes move with (the taste rules: music moves
// things through smoothed envelopes, nothing on every kick).

export const BANDS = 32;
const FFT = 4096;
const LOW_HZ = 50;
const HIGH_HZ = 16000;
const HOP = 1 / 60;

export class Analysis {
  constructor(context, source) {
    this.analyser = context.createAnalyser();
    this.analyser.fftSize = FFT;
    this.analyser.smoothingTimeConstant = 0;
    source.connect(this.analyser);
    this.rate = context.sampleRate;
    this.samples = new Float32Array(FFT);
    this.re = new Float32Array(FFT);
    this.im = new Float32Array(FFT);
    this.window = Float32Array.from({ length: FFT }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / FFT));
    this.edges = bandEdges(this.rate);
    this.levels = new Float32Array(BANDS);
    this.peak = 1e-4;
    this.bassSlow = 0;
    this.kick = 0;
    this.bass = 0;
    this.level = 0;
    // Slow envelopes: bass, mids, highs, level.
    this.env = [0, 0, 0, 0];
    // Fast envelopes for motion that follows the beat: the music's energy
    // and its highs, each over a quiet floor, quick to rise and slow to
    // fall, 0..1.
    this.drive = [0, 0];
    this.pending = 0;
  }

  // Advances by `dt` seconds of wall time: as many 60 Hz hops as are due,
  // then the envelopes. Paused playback decays everything to silence.
  update(dt, playing) {
    this.pending = Math.min(this.pending + dt, 0.2);
    while (this.pending >= HOP) {
      this.pending -= HOP;
      this.hop(playing);
    }
    const mids = mean(this.levels, 8, 20);
    const highs = mean(this.levels, 20, 32);
    const targets = [this.bass, mids, highs, this.level];
    const taus = [0.6, 0.9, 0.7, 2.0];
    for (let i = 0; i < 4; i++) {
      const k = 1 - Math.exp(-dt / taus[i]);
      this.env[i] += (targets[i] - this.env[i]) * k;
    }
    const clamp = (v) => Math.min(Math.max(v, 0), 1);
    const drives = [clamp((Math.max(this.level, this.bass * 0.85) - 0.2) / 0.55), clamp((highs - 0.15) / 0.5)];
    const rise = [0.08, 0.06];
    const fall = [0.7, 0.5];
    for (let i = 0; i < 2; i++) {
      const tau = drives[i] > this.drive[i] ? rise[i] : fall[i];
      this.drive[i] += (drives[i] - this.drive[i]) * (1 - Math.exp(-dt / tau));
    }
  }

  hop(playing) {
    if (playing) {
      this.analyser.getFloatTimeDomainData(this.samples);
    } else {
      this.samples.fill(0);
    }
    for (let i = 0; i < FFT; i++) {
      this.re[i] = this.samples[i] * this.window[i];
      this.im[i] = 0;
    }
    fft(this.re, this.im);
    const raw = new Float32Array(BANDS);
    for (let b = 0; b < BANDS; b++) {
      const [lo, hi] = this.edges[b];
      let sum = 0;
      for (let k = lo; k < hi; k++) {
        sum += (this.re[k] * this.re[k] + this.im[k] * this.im[k]) / FFT;
      }
      raw[b] = Math.sqrt(sum / (hi - lo));
    }
    let loudest = 0;
    for (const v of raw) loudest = Math.max(loudest, v);
    this.peak = Math.max(this.peak * 0.995, loudest, 1e-4);
    for (let b = 0; b < BANDS; b++) {
      const db = 20 * Math.log10(Math.max(raw[b] / this.peak, 1e-6));
      const target = Math.min(Math.max((db + 48) / 48, 0), 1);
      const speed = target > this.levels[b] ? 0.6 : 0.12;
      this.levels[b] += (target - this.levels[b]) * speed;
    }
    this.bass = mean(this.levels, 0, 4);
    const onset = Math.min(Math.max((this.bass - this.bassSlow) * 4, 0), 1);
    this.bassSlow += (this.bass - this.bassSlow) * 0.15;
    this.kick = Math.max(this.kick * 0.88, onset);
    this.level = mean(this.levels, 0, BANDS);
  }
}

function mean(values, from, to) {
  let sum = 0;
  for (let i = from; i < to; i++) sum += values[i];
  return sum / (to - from);
}

function bandEdges(rate) {
  const bin = (hz) => Math.min(Math.max(Math.round((hz / rate) * FFT), 1), FFT / 2);
  const f = (i) => LOW_HZ * Math.pow(HIGH_HZ / LOW_HZ, i / BANDS);
  return Array.from({ length: BANDS }, (_, b) => {
    const lo = bin(f(b));
    const hi = bin(f(b + 1));
    return [lo, Math.max(hi, lo + 1)];
  });
}

// In-place iterative radix-2 FFT.
export function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const angle = (-2 * Math.PI) / len;
    const wr = Math.cos(angle);
    const wi = Math.sin(angle);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k;
        const b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci;
        const ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr;
        im[b] = im[a] - ti;
        re[a] += tr;
        im[a] += ti;
        const next = cr * wr - ci * wi;
        ci = cr * wi + ci * wr;
        cr = next;
      }
    }
  }
}
