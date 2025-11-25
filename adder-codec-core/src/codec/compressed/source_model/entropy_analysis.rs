//! Entropy analysis tools for evaluating context-switching compression potential.
//!
//! This module provides tools to analyze the statistical distribution of D values
//! and T residuals across different scenarios, helping quantify potential compression
//! gains from context switching.

use crate::{DeltaT, Event, EventCoordless, D};
use std::collections::HashMap;

/// Statistics for a single context/scenario
#[derive(Debug, Clone, Default)]
pub struct ContextStats {
    /// Histogram of symbol frequencies
    pub histogram: HashMap<i32, u64>,
    /// Total number of symbols
    pub total_count: u64,
    /// Running sum for mean calculation
    sum: i64,
    /// Running sum of squares for variance
    sum_sq: i64,
}

impl ContextStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a symbol occurrence
    pub fn record(&mut self, symbol: i32) {
        *self.histogram.entry(symbol).or_insert(0) += 1;
        self.total_count += 1;
        self.sum += symbol as i64;
        self.sum_sq += (symbol as i64) * (symbol as i64);
    }

    /// Calculate the entropy in bits per symbol
    pub fn entropy(&self) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }

        let total = self.total_count as f64;
        let mut entropy = 0.0;

        for &count in self.histogram.values() {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Calculate mean
    pub fn mean(&self) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        self.sum as f64 / self.total_count as f64
    }

    /// Calculate variance
    pub fn variance(&self) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        let mean = self.mean();
        (self.sum_sq as f64 / self.total_count as f64) - (mean * mean)
    }

    /// Calculate standard deviation
    pub fn std_dev(&self) -> f64 {
        self.variance().sqrt()
    }

    /// Number of unique symbols
    pub fn unique_symbols(&self) -> usize {
        self.histogram.len()
    }
}

/// Classification for D value trends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DTrend {
    /// D is increasing (pixel getting brighter)
    Rising,
    /// D is decreasing (pixel getting dimmer)
    Falling,
    /// D is stable (small changes)
    Stable,
    /// First event for this pixel (no history)
    Initial,
}

/// Classification based on D magnitude
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DMagnitude {
    /// Low D values (0-42, ~1/3 of range)
    Low,
    /// Medium D values (43-84)
    Medium,
    /// High D values (85-127)
    High,
}

/// Classification based on event density
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventDensity {
    /// Few events at this pixel
    Sparse,
    /// Moderate events
    Normal,
    /// Many events (high activity)
    Dense,
}

/// Analyzer for measuring entropy across different potential contexts
#[derive(Debug)]
pub struct EntropyAnalyzer {
    /// Global D residual statistics (current approach)
    pub global_d_stats: ContextStats,

    /// Global T residual statistics (current approach)
    pub global_t_stats: ContextStats,

    /// D residual stats by trend
    pub d_by_trend: HashMap<DTrend, ContextStats>,

    /// D residual stats by magnitude of previous D
    pub d_by_prev_magnitude: HashMap<DMagnitude, ContextStats>,

    /// D residual stats by event density
    pub d_by_density: HashMap<EventDensity, ContextStats>,

    /// T residual stats by D magnitude
    pub t_by_d_magnitude: HashMap<DMagnitude, ContextStats>,

    /// T residual stats by trend
    pub t_by_trend: HashMap<DTrend, ContextStats>,

    /// Per-pixel state tracking
    pixel_history: HashMap<(u16, u16, u8), Vec<EventCoordless>>,

    /// Event counts per pixel for density classification
    pixel_event_counts: HashMap<(u16, u16, u8), u32>,

    /// Reference time interval
    dt_ref: DeltaT,
}

impl EntropyAnalyzer {
    pub fn new(dt_ref: DeltaT) -> Self {
        Self {
            global_d_stats: ContextStats::new(),
            global_t_stats: ContextStats::new(),
            d_by_trend: HashMap::new(),
            d_by_prev_magnitude: HashMap::new(),
            d_by_density: HashMap::new(),
            t_by_d_magnitude: HashMap::new(),
            t_by_trend: HashMap::new(),
            pixel_history: HashMap::new(),
            pixel_event_counts: HashMap::new(),
            dt_ref,
        }
    }

    /// Classify D value into magnitude bucket
    fn classify_d_magnitude(d: D) -> DMagnitude {
        match d {
            0..=42 => DMagnitude::Low,
            43..=84 => DMagnitude::Medium,
            _ => DMagnitude::High,
        }
    }

    /// Classify trend based on D residual
    fn classify_trend(d_residual: i32) -> DTrend {
        match d_residual {
            r if r > 3 => DTrend::Rising,
            r if r < -3 => DTrend::Falling,
            _ => DTrend::Stable,
        }
    }

    /// Classify density based on event count
    fn classify_density(count: u32) -> EventDensity {
        match count {
            0..=2 => EventDensity::Sparse,
            3..=10 => EventDensity::Normal,
            _ => EventDensity::Dense,
        }
    }

    /// Process an event and update statistics
    pub fn process_event(&mut self, event: &Event) {
        let pixel_key = (event.coord.x, event.coord.y, event.coord.c.unwrap_or(0));

        // Update event count for this pixel
        let count = self.pixel_event_counts.entry(pixel_key).or_insert(0);
        *count += 1;
        let density = Self::classify_density(*count);

        // Get pixel history
        let history = self.pixel_history.entry(pixel_key).or_insert_with(Vec::new);

        if let Some(prev) = history.last() {
            // Calculate residuals
            let d_residual = event.d as i32 - prev.d as i32;
            let t_residual = event.t as i64 - prev.t as i64;

            // Classify
            let prev_magnitude = Self::classify_d_magnitude(prev.d);
            let trend = Self::classify_trend(d_residual);

            // Record global stats
            self.global_d_stats.record(d_residual);
            self.global_t_stats.record(t_residual as i32);

            // Record by trend
            self.d_by_trend
                .entry(trend)
                .or_insert_with(ContextStats::new)
                .record(d_residual);

            // Record by previous magnitude
            self.d_by_prev_magnitude
                .entry(prev_magnitude)
                .or_insert_with(ContextStats::new)
                .record(d_residual);

            // Record by density
            self.d_by_density
                .entry(density)
                .or_insert_with(ContextStats::new)
                .record(d_residual);

            // Record T stats by D magnitude
            let cur_magnitude = Self::classify_d_magnitude(event.d);
            self.t_by_d_magnitude
                .entry(cur_magnitude)
                .or_insert_with(ContextStats::new)
                .record(t_residual as i32);

            // Record T stats by trend
            self.t_by_trend
                .entry(trend)
                .or_insert_with(ContextStats::new)
                .record(t_residual as i32);
        } else {
            // First event - record the absolute D value as "residual" from 0
            self.global_d_stats.record(event.d as i32);
            self.d_by_trend
                .entry(DTrend::Initial)
                .or_insert_with(ContextStats::new)
                .record(event.d as i32);
        }

        // Update history
        history.push(EventCoordless {
            d: event.d,
            t: event.t,
        });

        // Keep history bounded to save memory
        if history.len() > 10 {
            history.remove(0);
        }
    }

    /// Generate a comprehensive analysis report
    pub fn generate_report(&self) -> EntropyReport {
        let global_d_entropy = self.global_d_stats.entropy();
        let global_t_entropy = self.global_t_stats.entropy();

        // Calculate weighted average entropy for context-switched D
        let d_by_trend_entropy = self.weighted_entropy(&self.d_by_trend);
        let d_by_magnitude_entropy = self.weighted_entropy(&self.d_by_prev_magnitude);
        let d_by_density_entropy = self.weighted_entropy(&self.d_by_density);

        // Calculate weighted average entropy for context-switched T
        let t_by_magnitude_entropy = self.weighted_entropy(&self.t_by_d_magnitude);
        let t_by_trend_entropy = self.weighted_entropy(&self.t_by_trend);

        EntropyReport {
            total_events: self.global_d_stats.total_count,

            // D analysis
            global_d_entropy,
            global_d_mean: self.global_d_stats.mean(),
            global_d_std_dev: self.global_d_stats.std_dev(),
            global_d_unique_symbols: self.global_d_stats.unique_symbols(),

            d_by_trend_entropy,
            d_by_trend_potential_savings: (1.0 - d_by_trend_entropy / global_d_entropy) * 100.0,
            d_by_trend_details: self.context_details(&self.d_by_trend),

            d_by_magnitude_entropy,
            d_by_magnitude_potential_savings: (1.0 - d_by_magnitude_entropy / global_d_entropy)
                * 100.0,
            d_by_magnitude_details: self.context_details(&self.d_by_prev_magnitude),

            d_by_density_entropy,
            d_by_density_potential_savings: (1.0 - d_by_density_entropy / global_d_entropy) * 100.0,
            d_by_density_details: self.context_details(&self.d_by_density),

            // T analysis
            global_t_entropy,
            global_t_mean: self.global_t_stats.mean(),
            global_t_std_dev: self.global_t_stats.std_dev(),

            t_by_magnitude_entropy,
            t_by_magnitude_potential_savings: if global_t_entropy > 0.0 {
                (1.0 - t_by_magnitude_entropy / global_t_entropy) * 100.0
            } else {
                0.0
            },

            t_by_trend_entropy,
            t_by_trend_potential_savings: if global_t_entropy > 0.0 {
                (1.0 - t_by_trend_entropy / global_t_entropy) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Calculate weighted average entropy across contexts
    fn weighted_entropy<K: std::hash::Hash + Eq>(&self, contexts: &HashMap<K, ContextStats>) -> f64 {
        let total: u64 = contexts.values().map(|c| c.total_count).sum();
        if total == 0 {
            return 0.0;
        }

        contexts
            .values()
            .map(|c| {
                let weight = c.total_count as f64 / total as f64;
                weight * c.entropy()
            })
            .sum()
    }

    /// Get details for each context
    fn context_details<K: std::fmt::Debug + std::hash::Hash + Eq + Clone>(
        &self,
        contexts: &HashMap<K, ContextStats>,
    ) -> Vec<ContextDetail> {
        let mut details: Vec<_> = contexts
            .iter()
            .map(|(k, v)| ContextDetail {
                name: format!("{:?}", k),
                count: v.total_count,
                entropy: v.entropy(),
                mean: v.mean(),
                std_dev: v.std_dev(),
                unique_symbols: v.unique_symbols(),
            })
            .collect();

        details.sort_by(|a, b| b.count.cmp(&a.count));
        details
    }
}

/// Detail for a single context
#[derive(Debug, Clone)]
pub struct ContextDetail {
    pub name: String,
    pub count: u64,
    pub entropy: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub unique_symbols: usize,
}

/// Full entropy analysis report
#[derive(Debug)]
pub struct EntropyReport {
    pub total_events: u64,

    // D residual analysis
    pub global_d_entropy: f64,
    pub global_d_mean: f64,
    pub global_d_std_dev: f64,
    pub global_d_unique_symbols: usize,

    pub d_by_trend_entropy: f64,
    pub d_by_trend_potential_savings: f64,
    pub d_by_trend_details: Vec<ContextDetail>,

    pub d_by_magnitude_entropy: f64,
    pub d_by_magnitude_potential_savings: f64,
    pub d_by_magnitude_details: Vec<ContextDetail>,

    pub d_by_density_entropy: f64,
    pub d_by_density_potential_savings: f64,
    pub d_by_density_details: Vec<ContextDetail>,

    // T residual analysis
    pub global_t_entropy: f64,
    pub global_t_mean: f64,
    pub global_t_std_dev: f64,

    pub t_by_magnitude_entropy: f64,
    pub t_by_magnitude_potential_savings: f64,

    pub t_by_trend_entropy: f64,
    pub t_by_trend_potential_savings: f64,
}

impl std::fmt::Display for EntropyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "╔══════════════════════════════════════════════════════════════╗")?;
        writeln!(f, "║           ENTROPY ANALYSIS REPORT                            ║")?;
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ Total events analyzed: {:>38} ║", self.total_events)?;
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║                    D RESIDUAL ANALYSIS                       ║")?;
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(
            f,
            "║ Global entropy:     {:>8.4} bits/symbol                    ║",
            self.global_d_entropy
        )?;
        writeln!(
            f,
            "║ Global mean:        {:>8.4}                                 ║",
            self.global_d_mean
        )?;
        writeln!(
            f,
            "║ Global std dev:     {:>8.4}                                 ║",
            self.global_d_std_dev
        )?;
        writeln!(
            f,
            "║ Unique symbols:     {:>8}                                 ║",
            self.global_d_unique_symbols
        )?;
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║ Context switching potential (D residuals):                   ║")?;
        writeln!(f, "╟──────────────────────────────────────────────────────────────╢")?;
        writeln!(
            f,
            "║ By Trend:      {:>8.4} bits  ({:>+6.2}% savings)              ║",
            self.d_by_trend_entropy, self.d_by_trend_potential_savings
        )?;
        for detail in &self.d_by_trend_details {
            writeln!(
                f,
                "║   {:12} {:>8} events, entropy={:.4} bits            ║",
                detail.name, detail.count, detail.entropy
            )?;
        }
        writeln!(f, "╟──────────────────────────────────────────────────────────────╢")?;
        writeln!(
            f,
            "║ By Prev D Mag: {:>8.4} bits  ({:>+6.2}% savings)              ║",
            self.d_by_magnitude_entropy, self.d_by_magnitude_potential_savings
        )?;
        for detail in &self.d_by_magnitude_details {
            writeln!(
                f,
                "║   {:12} {:>8} events, entropy={:.4} bits            ║",
                detail.name, detail.count, detail.entropy
            )?;
        }
        writeln!(f, "╟──────────────────────────────────────────────────────────────╢")?;
        writeln!(
            f,
            "║ By Density:    {:>8.4} bits  ({:>+6.2}% savings)              ║",
            self.d_by_density_entropy, self.d_by_density_potential_savings
        )?;
        for detail in &self.d_by_density_details {
            writeln!(
                f,
                "║   {:12} {:>8} events, entropy={:.4} bits            ║",
                detail.name, detail.count, detail.entropy
            )?;
        }
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(f, "║                    T RESIDUAL ANALYSIS                       ║")?;
        writeln!(f, "╠══════════════════════════════════════════════════════════════╣")?;
        writeln!(
            f,
            "║ Global entropy:     {:>8.4} bits/symbol                    ║",
            self.global_t_entropy
        )?;
        writeln!(
            f,
            "║ Global mean:        {:>8.4}                                 ║",
            self.global_t_mean
        )?;
        writeln!(
            f,
            "║ Global std dev:     {:>8.4}                                 ║",
            self.global_t_std_dev
        )?;
        writeln!(f, "╟──────────────────────────────────────────────────────────────╢")?;
        writeln!(
            f,
            "║ By D Magnitude: {:>7.4} bits  ({:>+6.2}% savings)              ║",
            self.t_by_magnitude_entropy, self.t_by_magnitude_potential_savings
        )?;
        writeln!(
            f,
            "║ By Trend:       {:>7.4} bits  ({:>+6.2}% savings)              ║",
            self.t_by_trend_entropy, self.t_by_trend_potential_savings
        )?;
        writeln!(f, "╚══════════════════════════════════════════════════════════════╝")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Coord;

    #[test]
    fn test_entropy_calculation() {
        let mut stats = ContextStats::new();

        // Uniform distribution of 4 symbols: entropy should be 2 bits
        for _ in 0..100 {
            stats.record(0);
            stats.record(1);
            stats.record(2);
            stats.record(3);
        }

        let entropy = stats.entropy();
        assert!((entropy - 2.0).abs() < 0.01, "Expected ~2 bits, got {}", entropy);
    }

    #[test]
    fn test_skewed_distribution() {
        let mut stats = ContextStats::new();

        // Heavily skewed: mostly 0s
        for _ in 0..900 {
            stats.record(0);
        }
        for _ in 0..100 {
            stats.record(1);
        }

        let entropy = stats.entropy();
        // Should be much less than 1 bit
        assert!(entropy < 0.5, "Expected <0.5 bits, got {}", entropy);
    }

    #[test]
    fn test_analyzer_basic() {
        let mut analyzer = EntropyAnalyzer::new(255);

        // Simulate a sequence of events at one pixel
        for i in 0..100 {
            let event = Event {
                coord: Coord::new(10, 10, Some(0)),
                d: 7 + (i % 3) as u8, // D values: 7, 8, 9, 7, 8, 9...
                t: i * 100,
            };
            analyzer.process_event(&event);
        }

        let report = analyzer.generate_report();
        println!("{}", report);

        assert!(report.total_events > 0);
        assert!(report.global_d_entropy > 0.0);
    }
}
