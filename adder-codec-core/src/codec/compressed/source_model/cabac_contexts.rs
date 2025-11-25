use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::fenwick::Weights;
use crate::codec::compressed::{DRESIDUAL_NO_EVENT, DResidual};
use crate::{AbsoluteT, DeltaT, EventCoordless, Intensity, D, D_SHIFT};
use arithmetic_coding_adder_dep::Encoder;
use bitstream_io::{BigEndian, BitWrite, BitWriter};
use std::sync::atomic::AtomicU64;

// Global context usage statistics across all ADUs
pub(crate) static GLOBAL_D_INITIAL_COUNT: AtomicU64 = AtomicU64::new(0);
pub(crate) static GLOBAL_D_STABLE_COUNT: AtomicU64 = AtomicU64::new(0);
pub(crate) static GLOBAL_D_INC_COUNT: AtomicU64 = AtomicU64::new(0);
pub(crate) static GLOBAL_D_DEC_COUNT: AtomicU64 = AtomicU64::new(0);
pub(crate) static GLOBAL_D_NO_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);

pub struct Contexts {
    /// Decimation factor residuals context
    pub(crate) d_initial_context: usize,
    pub(crate) d_stable_context: usize, // Stable also stores NO_EVENT
    pub(crate) d_inc_context: usize,
    pub(crate) d_dec_context: usize,

    /// Timestamp residuals context
    pub(crate) t_context: usize,

    t_residual_max: i64,

    /// EOF context
    pub(crate) eof_context: usize,

    pub(crate) bitshift_context: usize,

    // Context usage tracking
    pub d_initial_count: std::sync::atomic::AtomicU64,
    pub d_stable_count: std::sync::atomic::AtomicU64,
    pub d_inc_count: std::sync::atomic::AtomicU64,
    pub d_dec_count: std::sync::atomic::AtomicU64,
    pub d_no_event_count: std::sync::atomic::AtomicU64,
}

pub const D_RESIDUAL_OFFSET: i16 = 255;

pub const BITSHIFT_ENCODE_FULL: u8 = 15;

impl Contexts {
    pub fn new(source_model: &mut FenwickModel, dt_ref: DeltaT) -> Self {
        let d_initial_context =
            source_model.push_context_with_weights(d_residual_default_weights());
        let d_stable_context = source_model.push_context_with_weights(d_residual_default_weights());
        let d_inc_context = source_model.push_context_with_weights(d_residual_default_weights());
        let d_dec_context = source_model.push_context_with_weights(d_residual_default_weights());

        // TODO: Configure this based on the delta_t_max parameter!!
        let t_weights = t_residual_default_weights(dt_ref);
        let t_residual_max = (t_weights.len() as i64 - 2) / 2;
        let t_context = source_model.push_context_with_weights(t_weights);

        let eof_context = source_model.push_context_with_weights(Weights::new_with_counts(1, &[1]));
        let bitshift_context =
            source_model.push_context_with_weights(Weights::new_with_counts(16, &[1; 16]));

        Self {
            d_initial_context,
            d_stable_context,
            d_inc_context,
            d_dec_context,
            t_context,
            t_residual_max,
            eof_context,
            bitshift_context,
            d_initial_count: std::sync::atomic::AtomicU64::new(0),
            d_stable_count: std::sync::atomic::AtomicU64::new(0),
            d_inc_count: std::sync::atomic::AtomicU64::new(0),
            d_dec_count: std::sync::atomic::AtomicU64::new(0),
            d_no_event_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Find out how much we need to bitshift the t_residual to fit within the range of the model
    pub(crate) fn residual_to_bitshift(&self, t_residual_i64: i64) -> (u8, i64) {
        if t_residual_i64.abs() < self.t_residual_max {
            (0, t_residual_i64)
            // } else if t_residual_i64.abs() > self.dt_max {
        } else {
            // JUST LOSSLESS FOR NOW
            (BITSHIFT_ENCODE_FULL, t_residual_i64)
        }
        // else {
        //     let mut bitshift = 0;
        //     let mut t_residual = t_residual_i64.abs();
        //     while t_residual > self.t_residual_max {
        //         t_residual >>= 1;
        //         bitshift += 1;
        //     }
        //     if t_residual_i64 < 0 {
        //         (bitshift, -t_residual)
        //     } else {
        //         (bitshift, t_residual)
        //     }
        // }
    }

    fn event_to_intensity(&self, d: D, delta_t: DeltaT, dt_ref: DeltaT) -> f64 {
        let intensity = match d as usize {
            a if a >= D_SHIFT.len() => f64::from(0),
            _ => match delta_t {
                0 => D_SHIFT[d as usize] as Intensity, // treat it as dt = 1
                _ => D_SHIFT[d as usize] as Intensity / f64::from(delta_t),
            },
        };
        intensity * dt_ref as f64
    }

    pub(crate) fn residual_to_bitshift2(
        &self,
        t_prediction: i64,
        t_residual_i64: i64,
        event: &mut EventCoordless,
        prev_event: &EventCoordless,
        dt_ref: DeltaT,
        c_thresh_max: f64,
    ) -> (u8, i64) {
        if t_residual_i64.abs() < self.t_residual_max {
            (0, t_residual_i64)
        } else {
            if event.t < prev_event.t {
                // dbg!(event.clone(), prev_event.clone());
            }

            let actual_dt = event.t.saturating_sub(prev_event.t);
            let actual_intensity = self.event_to_intensity(event.d, actual_dt, dt_ref);
            let mut recon_intensity = actual_intensity;
            let mut bitshift: u8 = 0;
            let mut t_residual = t_residual_i64.abs();
            loop {
                if t_residual > self.t_residual_max
                    && actual_intensity - c_thresh_max < recon_intensity
                    && actual_intensity + c_thresh_max > recon_intensity
                {
                    t_residual >>= 1;
                    bitshift += 1;
                    let recon_predicted_t = (t_prediction + t_residual) as AbsoluteT;
                    if recon_predicted_t < prev_event.t {
                        break;
                    }
                    let recon_predicted_dt = recon_predicted_t - prev_event.t;
                    recon_intensity = self.event_to_intensity(event.d, recon_predicted_dt, dt_ref);
                } else {
                    break;
                }
            }
            bitshift = bitshift.saturating_sub(1);
            t_residual = t_residual_i64.abs() >> bitshift;

            if t_residual.abs() < self.t_residual_max {
                if t_residual_i64 < 0 {
                    (bitshift, -t_residual)
                } else {
                    (bitshift, t_residual)
                }
            } else {
                // JUST LOSSLESS
                (BITSHIFT_ENCODE_FULL, t_residual_i64)
            }
        }
    }

    /// Get the D residual context based on the trend of previous D residuals, defaulting to stable
    pub(crate) fn d_trend_context(&self, last_d_residual: Option<DResidual>) -> usize {
        const THRESHOLD: DResidual = 10;
        match last_d_residual {
            Some(d) if d == DRESIDUAL_NO_EVENT => {
                // Track both local and global
                self.d_stable_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                GLOBAL_D_STABLE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.d_stable_context
            }
            Some(d) if d > THRESHOLD => {
                self.d_inc_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                GLOBAL_D_INC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.d_inc_context
            }
            Some(d) if d < -THRESHOLD => {
                self.d_dec_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                GLOBAL_D_DEC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.d_dec_context
            }
            _ => {
                self.d_stable_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                GLOBAL_D_STABLE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.d_stable_context
            }
        }
    }

    /// Print context usage statistics for this ADU
    pub fn print_context_stats(&self) {
        let initial = self.d_initial_count.load(std::sync::atomic::Ordering::Relaxed);
        let stable = self.d_stable_count.load(std::sync::atomic::Ordering::Relaxed);
        let inc = self.d_inc_count.load(std::sync::atomic::Ordering::Relaxed);
        let dec = self.d_dec_count.load(std::sync::atomic::Ordering::Relaxed);
        let total = initial + stable + inc + dec;

        if total > 0 {
            println!("\n=== D CONTEXT USAGE (This ADU) ===");
            println!("d_initial_context: {:6} ({:5.2}%)", initial, initial as f64 / total as f64 * 100.0);
            println!("d_stable_context:  {:6} ({:5.2}%)", stable, stable as f64 / total as f64 * 100.0);
            println!("d_inc_context:     {:6} ({:5.2}%)", inc, inc as f64 / total as f64 * 100.0);
            println!("d_dec_context:     {:6} ({:5.2}%)", dec, dec as f64 / total as f64 * 100.0);
            println!("Total D symbols:   {}", total);
            println!("===================================\n");
        }
    }
}

/// Reset global context usage statistics
pub fn reset_global_context_stats() {
    GLOBAL_D_INITIAL_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    GLOBAL_D_STABLE_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    GLOBAL_D_INC_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    GLOBAL_D_DEC_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    GLOBAL_D_NO_EVENT_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Print global context usage statistics across all ADUs
pub fn print_global_context_stats() {
    let initial = GLOBAL_D_INITIAL_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let stable = GLOBAL_D_STABLE_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let inc = GLOBAL_D_INC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let dec = GLOBAL_D_DEC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let no_event = GLOBAL_D_NO_EVENT_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let total = initial + stable + inc + dec;

    if total > 0 {
        println!("\n=== D CONTEXT USAGE (ALL ADUs) ===");
        println!("d_initial_context: {:6} ({:5.2}%)", initial, initial as f64 / total as f64 * 100.0);
        println!("d_stable_context:  {:6} ({:5.2}%) [includes NO_EVENT]", stable, stable as f64 / total as f64 * 100.0);
        println!("  └─ NO_EVENT:     {:6} ({:5.2}% of stable, {:5.2}% of total)",
                 no_event,
                 if stable > 0 { no_event as f64 / stable as f64 * 100.0 } else { 0.0 },
                 no_event as f64 / total as f64 * 100.0);
        println!("d_inc_context:     {:6} ({:5.2}%)", inc, inc as f64 / total as f64 * 100.0);
        println!("d_dec_context:     {:6} ({:5.2}%)", dec, dec as f64 / total as f64 * 100.0);
        println!("Total D symbols:   {}", total);
        println!("===================================\n");
    }
}


pub fn t_residual_default_weights(_dt_ref: DeltaT) -> Weights {
    // t residuals can fit within i16

    // After we've indexed into the correct interval, our timestamp residual can span [-dt_ref, dt_ref]

    // We have dt_max/dt_ref count of intervals per adu
    let mut counts: Vec<u64> = vec![1; u8::MAX as usize + 1];

    // Give higher probability to smaller residuals
    // for i in counts.len() / 3..counts.len() * 2 / 3 {
    //     counts[i] = 5;
    // }
    // let len = counts.len();
    // counts[len / 2] = 10;
    // counts[len / 2 - 1] = 10;
    // counts[len / 2 + 1] = 10;
    counts[0] = 100;
    for item in counts.iter_mut().take(10) {
        *item = 10;
    }

    Weights::new_with_counts(counts.len(), &counts)
}

// pub fn dtref_residual_default_weights(dt_ref: DeltaT, dt_max: DeltaT) -> Weights {
//     // dtref residuals can fit within i16
//
//     // We have dt_max/dt_ref count of intervals per adu
//     let mut counts: Vec<u64> = vec![1; (dt_max / dt_ref) as usize * 2 + 1];
//
//     // Give higher probability to smaller residuals
//     for i in counts.len() / 3..counts.len() * 2 / 3 {
//         counts[i] = 5;
//     }
//
//     let len = counts.len();
//     counts[len / 2] = 10;
//
//     Weights::new_with_counts(counts.len(), &counts)
// }

pub fn d_residual_default_weights() -> Weights {
    // d residuals can fit within i16

    // DResidual_NO_EVENT =  256
    // DResidual_SKIP_CUBE =  257
    // The maximum positive d residual is d = 0 --> d = 255      [255]
    // The maximum negative d residual is d = 255 --> d = 0      [-255]
    // No d values in range (D_MAX, D_NO_EVENT) --> (173, 253)

    // Span the range [-255, 257]
    let mut counts: [u64; 513] = [1; 513];

    // Give high probability to range [-20, 20]
    let mut idx = 0;
    loop {
        match idx {
            // [-10, 10]
            245..=265 => {
                counts[idx] = 20;
            }

            // [-20, 20]
            235..=275 | 490..=510 | 0..=20 => {
                counts[idx] = 10;
            }

            // give high probability to d_no_event
            511 => {
                counts[idx] = 20;
            }

            // give high probability to skip cube
            512 => {
                counts[idx] = 10;
            }
            _ => {}
        }

        if idx == counts.len() - 1 {
            break;
        }

        idx += 1;
    }

    Weights::new_with_counts(counts.len(), &Vec::from(counts))
}

pub fn eof_context(
    contexts: &Contexts,
    encoder: &mut Encoder<FenwickModel, BitWriter<Vec<u8>, BigEndian>>,
    stream: &mut BitWriter<Vec<u8>, BigEndian>,
) {
    // THIS IS CRUCIAL FOR TESTING
    let eof_context = contexts.eof_context;
    encoder.model.set_context(eof_context);
    encoder.encode(None, stream).unwrap();
    encoder.flush(stream).unwrap();
    stream.byte_align().unwrap();
    stream.flush().unwrap();
}
