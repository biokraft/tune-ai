//! Port of `rosic_FourierTransformerRadix2` restricted to the real-signal
//! path the wavetable uses, plus the routines of Ooura's `fft4g.c` that path
//! calls (`rdft`, `makewt`, `makect`, `bitrv2`, `cftfsub`, `cftbsub`,
//! `cft1st`, `cftmdl`, `rftfsub`, `rftbsub`).
//!
//! `fft4g.c` is Takuya Ooura's General Purpose FFT Package
//! (<https://www.kurims.kyoto-u.ac.jp/~ooura/fft.html>); this is a modified
//! (ported) version, see `THIRD_PARTY.md`.
//!
//! The C++ passes pointer offsets (`ip + 2`, `w + nw`); here those are slices.
//! All state lives in the instance, so two transformers never interfere.

/// `FourierTransformerRadix2::directions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Forward,
    Inverse,
}

/// `rosic::FourierTransformerRadix2` (real-signal subset, default
/// `NORMALIZE_ON_INVERSE_TRAFO` normalization).
#[derive(Clone, Debug)]
pub struct FourierTransformerRadix2 {
    n: usize,
    direction: Direction,
    normalization_factor: f64,
    /// Twiddle factors, `2 * n` entries.
    w: Vec<f64>,
    /// Work area of Ooura's routines; `ip[0] == 0` requests re-initialization.
    ip: Vec<i32>,
}

impl FourierTransformerRadix2 {
    /// Constructor with block size `n` (a power of two, at least 2); the C++
    /// default constructor sets 256 and callers then call `setBlockSize`.
    pub fn new(n: usize) -> Self {
        assert!(n >= 2 && n.is_power_of_two());
        let mut s = Self {
            n,
            direction: Direction::Forward,
            normalization_factor: 1.0,
            w: vec![0.0; 2 * n],
            ip: vec![0; (4.0 + (n as f64).sqrt()).ceil() as usize],
        };
        s.ip[0] = 0; // indicate that re-initialization is necessary
        s.update_normalization_factor();
        s
    }

    fn set_direction(&mut self, new_direction: Direction) {
        if new_direction != self.direction {
            self.direction = new_direction;
            self.update_normalization_factor();
        }
    }

    /// `updateNormalizationFactor` for `NORMALIZE_ON_INVERSE_TRAFO`.
    fn update_normalization_factor(&mut self) {
        self.normalization_factor = if self.direction == Direction::Inverse {
            1.0 / self.n as f64
        } else {
            1.0
        };
    }

    /// `transformRealSignal(double*, double*)`: forward transform of the
    /// real `signal` (length `n`) into `re_and_im` (interleaved, length `n`).
    pub fn transform_real_signal(&mut self, signal: &[f64], re_and_im: &mut [f64]) {
        self.set_direction(Direction::Forward);
        let n = self.n;
        if self.normalization_factor != 1.0 {
            for i in 0..n {
                re_and_im[i] = signal[i] * self.normalization_factor;
            }
        } else {
            re_and_im[..n].copy_from_slice(&signal[..n]);
        }
        rdft(n, 1, re_and_im, &mut self.ip, &mut self.w);
        // The routine returns the conjugate values; take them back.
        let mut i = 3;
        while i < n {
            re_and_im[i] = -re_and_im[i];
            i += 2;
        }
    }

    /// `transformSymmetricSpectrum(double*, double*)`: inverse transform of
    /// the interleaved half spectrum `re_and_im` into the real `signal`.
    pub fn transform_symmetric_spectrum(&mut self, re_and_im: &[f64], signal: &mut [f64]) {
        self.set_direction(Direction::Inverse);
        let n = self.n;
        if self.normalization_factor != 1.0 {
            for i in 0..n {
                signal[i] = 2.0 * re_and_im[i] * self.normalization_factor;
            }
        } else {
            for i in 0..n {
                signal[i] = 2.0 * re_and_im[i];
            }
        }
        let mut i = 3;
        while i < n {
            signal[i] = -signal[i];
            i += 2;
        }
        rdft(n, -1, signal, &mut self.ip, &mut self.w);
    }
}

/// `(sin(x), cos(x))` of one argument.
///
/// The C++ calls `sin(x)` and `cos(x)` back to back in `makewt`/`makect`;
/// clang on macOS merges that pair into one `__sincos_stret` call, whose
/// results differ from the separate libm calls in the last bit for some
/// arguments. Calling the same routine keeps the twiddle factors, and so the
/// whole transform, bit-identical to the reference.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn sin_cos_pair(x: f64) -> (f64, f64) {
    #[repr(C)]
    struct SinCos {
        sin: f64,
        cos: f64,
    }
    unsafe extern "C" {
        fn __sincos_stret(x: f64) -> SinCos;
    }
    // SAFETY: pure libSystem function of one double, returned in registers.
    let r = unsafe { __sincos_stret(x) };
    (r.sin, r.cos)
}

/// Portable fallback (not bit-identical to the macOS reference build).
#[cfg(not(target_os = "macos"))]
fn sin_cos_pair(x: f64) -> (f64, f64) {
    (x.sin(), x.cos())
}

fn swap2(a: &mut [f64], j1: usize, k1: usize) {
    let (xr, xi) = (a[j1], a[j1 + 1]);
    let (yr, yi) = (a[k1], a[k1 + 1]);
    a[j1] = yr;
    a[j1 + 1] = yi;
    a[k1] = xr;
    a[k1 + 1] = xi;
}

fn rdft(n: usize, isgn: i32, a: &mut [f64], ip: &mut [i32], w: &mut [f64]) {
    let mut nw = ip[0] as usize;
    if n > (nw << 2) {
        nw = n >> 2;
        makewt(nw, ip, w);
    }
    let mut nc = ip[1] as usize;
    if n > (nc << 2) {
        nc = n >> 2;
        makect(nc, ip, &mut w[nw..]);
    }
    if isgn >= 0 {
        if n > 4 {
            bitrv2(n, &mut ip[2..], a);
            cftfsub(n, a, w);
            rftfsub(n, a, nc, &w[nw..]);
        } else if n == 4 {
            cftfsub(n, a, w);
        }
        let xi = a[0] - a[1];
        a[0] += a[1];
        a[1] = xi;
    } else {
        a[1] = 0.5 * (a[0] - a[1]);
        a[0] -= a[1];
        if n > 4 {
            rftbsub(n, a, nc, &w[nw..]);
            bitrv2(n, &mut ip[2..], a);
            cftbsub(n, a, w);
        } else if n == 4 {
            cftfsub(n, a, w);
        }
    }
}

fn makewt(nw: usize, ip: &mut [i32], w: &mut [f64]) {
    ip[0] = nw as i32;
    ip[1] = 1;
    if nw > 2 {
        let nwh = nw >> 1;
        let delta = 1.0f64.atan() / nwh as f64;
        w[0] = 1.0;
        w[1] = 0.0;
        w[nwh] = (delta * nwh as f64).cos();
        w[nwh + 1] = w[nwh];
        if nwh > 2 {
            let mut j = 2;
            while j < nwh {
                let (y, x) = sin_cos_pair(delta * j as f64);
                w[j] = x;
                w[j + 1] = y;
                w[nw - j] = y;
                w[nw - j + 1] = x;
                j += 2;
            }
            bitrv2(nw, &mut ip[2..], w);
        }
    }
}

fn makect(nc: usize, ip: &mut [i32], c: &mut [f64]) {
    ip[1] = nc as i32;
    if nc > 1 {
        let nch = nc >> 1;
        let delta = 1.0f64.atan() / nch as f64;
        c[0] = (delta * nch as f64).cos();
        c[nch] = 0.5 * c[0];
        for j in 1..nch {
            let (sn, cs) = sin_cos_pair(delta * j as f64);
            c[j] = 0.5 * cs;
            c[nc - j] = 0.5 * sn;
        }
    }
}

fn bitrv2(n: usize, ip: &mut [i32], a: &mut [f64]) {
    ip[0] = 0;
    let mut l = n;
    let mut m = 1usize;
    while (m << 3) < l {
        l >>= 1;
        for j in 0..m {
            ip[m + j] = ip[j] + l as i32;
        }
        m <<= 1;
    }
    let m2 = 2 * m;
    if (m << 3) == l {
        for k in 0..m {
            for j in 0..k {
                let mut j1 = 2 * j + ip[k] as usize;
                let mut k1 = 2 * k + ip[j] as usize;
                swap2(a, j1, k1);
                j1 += m2;
                k1 += 2 * m2;
                swap2(a, j1, k1);
                j1 += m2;
                k1 -= m2;
                swap2(a, j1, k1);
                j1 += m2;
                k1 += 2 * m2;
                swap2(a, j1, k1);
            }
            let j1 = 2 * k + m2 + ip[k] as usize;
            let k1 = j1 + m2;
            swap2(a, j1, k1);
        }
    } else {
        for k in 1..m {
            for j in 0..k {
                let mut j1 = 2 * j + ip[k] as usize;
                let mut k1 = 2 * k + ip[j] as usize;
                swap2(a, j1, k1);
                j1 += m2;
                k1 += m2;
                swap2(a, j1, k1);
            }
        }
    }
}

fn cftfsub(n: usize, a: &mut [f64], w: &[f64]) {
    let mut l = 2;
    if n > 8 {
        cft1st(n, a, w);
        l = 8;
        while (l << 2) < n {
            cftmdl(n, l, a, w);
            l <<= 2;
        }
    }
    if (l << 2) == n {
        let mut j = 0;
        while j < l {
            let j1 = j + l;
            let j2 = j1 + l;
            let j3 = j2 + l;
            let x0r = a[j] + a[j1];
            let x0i = a[j + 1] + a[j1 + 1];
            let x1r = a[j] - a[j1];
            let x1i = a[j + 1] - a[j1 + 1];
            let x2r = a[j2] + a[j3];
            let x2i = a[j2 + 1] + a[j3 + 1];
            let x3r = a[j2] - a[j3];
            let x3i = a[j2 + 1] - a[j3 + 1];
            a[j] = x0r + x2r;
            a[j + 1] = x0i + x2i;
            a[j2] = x0r - x2r;
            a[j2 + 1] = x0i - x2i;
            a[j1] = x1r - x3i;
            a[j1 + 1] = x1i + x3r;
            a[j3] = x1r + x3i;
            a[j3 + 1] = x1i - x3r;
            j += 2;
        }
    } else {
        let mut j = 0;
        while j < l {
            let j1 = j + l;
            let x0r = a[j] - a[j1];
            let x0i = a[j + 1] - a[j1 + 1];
            a[j] += a[j1];
            a[j + 1] += a[j1 + 1];
            a[j1] = x0r;
            a[j1 + 1] = x0i;
            j += 2;
        }
    }
}

fn cftbsub(n: usize, a: &mut [f64], w: &[f64]) {
    let mut l = 2;
    if n > 8 {
        cft1st(n, a, w);
        l = 8;
        while (l << 2) < n {
            cftmdl(n, l, a, w);
            l <<= 2;
        }
    }
    if (l << 2) == n {
        let mut j = 0;
        while j < l {
            let j1 = j + l;
            let j2 = j1 + l;
            let j3 = j2 + l;
            let x0r = a[j] + a[j1];
            let x0i = -a[j + 1] - a[j1 + 1];
            let x1r = a[j] - a[j1];
            let x1i = -a[j + 1] + a[j1 + 1];
            let x2r = a[j2] + a[j3];
            let x2i = a[j2 + 1] + a[j3 + 1];
            let x3r = a[j2] - a[j3];
            let x3i = a[j2 + 1] - a[j3 + 1];
            a[j] = x0r + x2r;
            a[j + 1] = x0i - x2i;
            a[j2] = x0r - x2r;
            a[j2 + 1] = x0i + x2i;
            a[j1] = x1r - x3i;
            a[j1 + 1] = x1i - x3r;
            a[j3] = x1r + x3i;
            a[j3 + 1] = x1i + x3r;
            j += 2;
        }
    } else {
        let mut j = 0;
        while j < l {
            let j1 = j + l;
            let x0r = a[j] - a[j1];
            let x0i = -a[j + 1] + a[j1 + 1];
            a[j] += a[j1];
            a[j + 1] = -a[j + 1] - a[j1 + 1];
            a[j1] = x0r;
            a[j1 + 1] = x0i;
            j += 2;
        }
    }
}

fn cft1st(n: usize, a: &mut [f64], w: &[f64]) {
    let mut x0r = a[0] + a[2];
    let mut x0i = a[1] + a[3];
    let mut x1r = a[0] - a[2];
    let mut x1i = a[1] - a[3];
    let mut x2r = a[4] + a[6];
    let mut x2i = a[5] + a[7];
    let mut x3r = a[4] - a[6];
    let mut x3i = a[5] - a[7];
    a[0] = x0r + x2r;
    a[1] = x0i + x2i;
    a[4] = x0r - x2r;
    a[5] = x0i - x2i;
    a[2] = x1r - x3i;
    a[3] = x1i + x3r;
    a[6] = x1r + x3i;
    a[7] = x1i - x3r;
    let mut wk1r = w[2];
    x0r = a[8] + a[10];
    x0i = a[9] + a[11];
    x1r = a[8] - a[10];
    x1i = a[9] - a[11];
    x2r = a[12] + a[14];
    x2i = a[13] + a[15];
    x3r = a[12] - a[14];
    x3i = a[13] - a[15];
    a[8] = x0r + x2r;
    a[9] = x0i + x2i;
    a[12] = x2i - x0i;
    a[13] = x0r - x2r;
    x0r = x1r - x3i;
    x0i = x1i + x3r;
    a[10] = wk1r * (x0r - x0i);
    a[11] = wk1r * (x0r + x0i);
    x0r = x3i + x1r;
    x0i = x3r - x1i;
    a[14] = wk1r * (x0i - x0r);
    a[15] = wk1r * (x0i + x0r);
    let mut k1 = 0;
    let mut j = 16;
    while j < n {
        k1 += 2;
        let k2 = 2 * k1;
        let wk2r = w[k1];
        let wk2i = w[k1 + 1];
        wk1r = w[k2];
        let mut wk1i = w[k2 + 1];
        let mut wk3r = wk1r - 2.0 * wk2i * wk1i;
        let mut wk3i = 2.0 * wk2i * wk1r - wk1i;
        x0r = a[j] + a[j + 2];
        x0i = a[j + 1] + a[j + 3];
        x1r = a[j] - a[j + 2];
        x1i = a[j + 1] - a[j + 3];
        x2r = a[j + 4] + a[j + 6];
        x2i = a[j + 5] + a[j + 7];
        x3r = a[j + 4] - a[j + 6];
        x3i = a[j + 5] - a[j + 7];
        a[j] = x0r + x2r;
        a[j + 1] = x0i + x2i;
        x0r -= x2r;
        x0i -= x2i;
        a[j + 4] = wk2r * x0r - wk2i * x0i;
        a[j + 5] = wk2r * x0i + wk2i * x0r;
        x0r = x1r - x3i;
        x0i = x1i + x3r;
        a[j + 2] = wk1r * x0r - wk1i * x0i;
        a[j + 3] = wk1r * x0i + wk1i * x0r;
        x0r = x1r + x3i;
        x0i = x1i - x3r;
        a[j + 6] = wk3r * x0r - wk3i * x0i;
        a[j + 7] = wk3r * x0i + wk3i * x0r;
        wk1r = w[k2 + 2];
        wk1i = w[k2 + 3];
        wk3r = wk1r - 2.0 * wk2r * wk1i;
        wk3i = 2.0 * wk2r * wk1r - wk1i;
        x0r = a[j + 8] + a[j + 10];
        x0i = a[j + 9] + a[j + 11];
        x1r = a[j + 8] - a[j + 10];
        x1i = a[j + 9] - a[j + 11];
        x2r = a[j + 12] + a[j + 14];
        x2i = a[j + 13] + a[j + 15];
        x3r = a[j + 12] - a[j + 14];
        x3i = a[j + 13] - a[j + 15];
        a[j + 8] = x0r + x2r;
        a[j + 9] = x0i + x2i;
        x0r -= x2r;
        x0i -= x2i;
        a[j + 12] = -wk2i * x0r - wk2r * x0i;
        a[j + 13] = -wk2i * x0i + wk2r * x0r;
        x0r = x1r - x3i;
        x0i = x1i + x3r;
        a[j + 10] = wk1r * x0r - wk1i * x0i;
        a[j + 11] = wk1r * x0i + wk1i * x0r;
        x0r = x1r + x3i;
        x0i = x1i - x3r;
        a[j + 14] = wk3r * x0r - wk3i * x0i;
        a[j + 15] = wk3r * x0i + wk3i * x0r;
        j += 16;
    }
}

fn cftmdl(n: usize, l: usize, a: &mut [f64], w: &[f64]) {
    let m = l << 2;
    let mut j = 0;
    while j < l {
        let j1 = j + l;
        let j2 = j1 + l;
        let j3 = j2 + l;
        let x0r = a[j] + a[j1];
        let x0i = a[j + 1] + a[j1 + 1];
        let x1r = a[j] - a[j1];
        let x1i = a[j + 1] - a[j1 + 1];
        let x2r = a[j2] + a[j3];
        let x2i = a[j2 + 1] + a[j3 + 1];
        let x3r = a[j2] - a[j3];
        let x3i = a[j2 + 1] - a[j3 + 1];
        a[j] = x0r + x2r;
        a[j + 1] = x0i + x2i;
        a[j2] = x0r - x2r;
        a[j2 + 1] = x0i - x2i;
        a[j1] = x1r - x3i;
        a[j1 + 1] = x1i + x3r;
        a[j3] = x1r + x3i;
        a[j3 + 1] = x1i - x3r;
        j += 2;
    }
    let mut wk1r = w[2];
    j = m;
    while j < l + m {
        let j1 = j + l;
        let j2 = j1 + l;
        let j3 = j2 + l;
        let mut x0r = a[j] + a[j1];
        let mut x0i = a[j + 1] + a[j1 + 1];
        let x1r = a[j] - a[j1];
        let x1i = a[j + 1] - a[j1 + 1];
        let x2r = a[j2] + a[j3];
        let x2i = a[j2 + 1] + a[j3 + 1];
        let x3r = a[j2] - a[j3];
        let x3i = a[j2 + 1] - a[j3 + 1];
        a[j] = x0r + x2r;
        a[j + 1] = x0i + x2i;
        a[j2] = x2i - x0i;
        a[j2 + 1] = x0r - x2r;
        x0r = x1r - x3i;
        x0i = x1i + x3r;
        a[j1] = wk1r * (x0r - x0i);
        a[j1 + 1] = wk1r * (x0r + x0i);
        x0r = x3i + x1r;
        x0i = x3r - x1i;
        a[j3] = wk1r * (x0i - x0r);
        a[j3 + 1] = wk1r * (x0i + x0r);
        j += 2;
    }
    let mut k1 = 0;
    let m2 = 2 * m;
    let mut k = m2;
    while k < n {
        k1 += 2;
        let k2 = 2 * k1;
        let wk2r = w[k1];
        let wk2i = w[k1 + 1];
        wk1r = w[k2];
        let mut wk1i = w[k2 + 1];
        let mut wk3r = wk1r - 2.0 * wk2i * wk1i;
        let mut wk3i = 2.0 * wk2i * wk1r - wk1i;
        j = k;
        while j < l + k {
            let j1 = j + l;
            let j2 = j1 + l;
            let j3 = j2 + l;
            let mut x0r = a[j] + a[j1];
            let mut x0i = a[j + 1] + a[j1 + 1];
            let x1r = a[j] - a[j1];
            let x1i = a[j + 1] - a[j1 + 1];
            let x2r = a[j2] + a[j3];
            let x2i = a[j2 + 1] + a[j3 + 1];
            let x3r = a[j2] - a[j3];
            let x3i = a[j2 + 1] - a[j3 + 1];
            a[j] = x0r + x2r;
            a[j + 1] = x0i + x2i;
            x0r -= x2r;
            x0i -= x2i;
            a[j2] = wk2r * x0r - wk2i * x0i;
            a[j2 + 1] = wk2r * x0i + wk2i * x0r;
            x0r = x1r - x3i;
            x0i = x1i + x3r;
            a[j1] = wk1r * x0r - wk1i * x0i;
            a[j1 + 1] = wk1r * x0i + wk1i * x0r;
            x0r = x1r + x3i;
            x0i = x1i - x3r;
            a[j3] = wk3r * x0r - wk3i * x0i;
            a[j3 + 1] = wk3r * x0i + wk3i * x0r;
            j += 2;
        }
        wk1r = w[k2 + 2];
        wk1i = w[k2 + 3];
        wk3r = wk1r - 2.0 * wk2r * wk1i;
        wk3i = 2.0 * wk2r * wk1r - wk1i;
        j = k + m;
        while j < l + (k + m) {
            let j1 = j + l;
            let j2 = j1 + l;
            let j3 = j2 + l;
            let mut x0r = a[j] + a[j1];
            let mut x0i = a[j + 1] + a[j1 + 1];
            let x1r = a[j] - a[j1];
            let x1i = a[j + 1] - a[j1 + 1];
            let x2r = a[j2] + a[j3];
            let x2i = a[j2 + 1] + a[j3 + 1];
            let x3r = a[j2] - a[j3];
            let x3i = a[j2 + 1] - a[j3 + 1];
            a[j] = x0r + x2r;
            a[j + 1] = x0i + x2i;
            x0r -= x2r;
            x0i -= x2i;
            a[j2] = -wk2i * x0r - wk2r * x0i;
            a[j2 + 1] = -wk2i * x0i + wk2r * x0r;
            x0r = x1r - x3i;
            x0i = x1i + x3r;
            a[j1] = wk1r * x0r - wk1i * x0i;
            a[j1 + 1] = wk1r * x0i + wk1i * x0r;
            x0r = x1r + x3i;
            x0i = x1i - x3r;
            a[j3] = wk3r * x0r - wk3i * x0i;
            a[j3 + 1] = wk3r * x0i + wk3i * x0r;
            j += 2;
        }
        k += m2;
    }
}

fn rftfsub(n: usize, a: &mut [f64], nc: usize, c: &[f64]) {
    let m = n >> 1;
    let ks = 2 * nc / m;
    let mut kk = 0;
    let mut j = 2;
    while j < m {
        let k = n - j;
        kk += ks;
        let wkr = 0.5 - c[nc - kk];
        let wki = c[kk];
        let xr = a[j] - a[k];
        let xi = a[j + 1] + a[k + 1];
        let yr = wkr * xr - wki * xi;
        let yi = wkr * xi + wki * xr;
        a[j] -= yr;
        a[j + 1] -= yi;
        a[k] += yr;
        a[k + 1] -= yi;
        j += 2;
    }
}

fn rftbsub(n: usize, a: &mut [f64], nc: usize, c: &[f64]) {
    a[1] = -a[1];
    let m = n >> 1;
    let ks = 2 * nc / m;
    let mut kk = 0;
    let mut j = 2;
    while j < m {
        let k = n - j;
        kk += ks;
        let wkr = 0.5 - c[nc - kk];
        let wki = c[kk];
        let xr = a[j] - a[k];
        let xi = a[j + 1] + a[k + 1];
        let yr = wkr * xr + wki * xi;
        let yi = wkr * xi - wki * xr;
        a[j] -= yr;
        a[j + 1] = yi - a[j + 1];
        a[k] += yr;
        a[k + 1] = yi - a[k + 1];
        j += 2;
    }
    a[m + 1] = -a[m + 1];
}
