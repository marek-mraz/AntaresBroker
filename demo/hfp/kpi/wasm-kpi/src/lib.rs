//! Bento `wasm` processor plugin: public-transport KPIs that the NGSI-LD
//! temporal API cannot express.
//!
//! CIM 009 clause 4.5.19.1 defines exactly eight aggregation methods
//! (avg, distinctCount, max, min, stddev, sum, sumsq, totalCount), all applied
//! per Entity per Attribute per time bucket. That covers "mean speed of bus 7
//! last hour" and nothing here:
//!
//!   * p95 speed        — percentiles are not in the aggrMethods vocabulary
//!   * mean headway     — needs consecutive arrivals ACROSS entities, ordered
//!   * bunching count   — ordering-dependent, threshold over adjacent gaps
//!
//! Host ABI (bento internal/impl/wasm/processor_wazero.go): the host allocates
//! inbound bytes with our `allocate`, calls `process` with no arguments, and
//! frees the outbound buffer with `deallocate`. We read the message through the
//! `bento_wasm` import module and write the result back the same way.

use std::alloc::{alloc, dealloc, Layout};
use std::collections::BTreeMap;

#[link(wasm_import_module = "bento_wasm")]
extern "C" {
    /// Returns the inbound message as a packed (ptr << 32) | len pair.
    fn v0_msg_as_bytes() -> u64;
    /// Hands the outbound message back to the host.
    fn v0_msg_set_bytes(ptr: u32, size: u32);
}

/// Exact-size allocation so `deallocate` can rebuild an identical Layout.
/// A `Vec::with_capacity` here would be wrong: capacity may exceed `size`.
fn layout(size: u32) -> Layout {
    // align 1: the host only ever writes/reads raw bytes through this pointer.
    unsafe { Layout::from_size_align_unchecked(size as usize, 1) }
}

#[no_mangle]
pub extern "C" fn allocate(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    unsafe { alloc(layout(size)) as u32 }
}

#[no_mangle]
pub extern "C" fn deallocate(ptr: u32, size: u32) {
    if ptr == 0 || size == 0 {
        return;
    }
    unsafe { dealloc(ptr as *mut u8, layout(size)) }
}

#[derive(serde::Deserialize)]
struct Sample {
    line: String,
    #[serde(default)]
    speed: Option<f64>,
    /// Seconds since epoch, already normalised by the Bloblang stage.
    #[serde(default)]
    observed: Option<f64>,
    #[serde(default)]
    at_stop: Option<String>,
    /// HFP schedule deviation in seconds. NEGATIVE means running late,
    /// positive means ahead of schedule — the sign convention is HFP's, and
    /// the existing demo chart labels it the same way.
    #[serde(default)]
    delay: Option<f64>,
}

#[derive(serde::Serialize)]
struct Kpi {
    line: String,
    /// Vehicles currently reporting on this line. Kept separate from
    /// `arrival_count`: the two input streams are concatenated, so a single
    /// total silently mixes 18 trams with 281 arrival events and means nothing.
    #[serde(rename = "vehicleCount")]
    vehicle_count: usize,
    #[serde(rename = "arrivalCount")]
    arrival_count: usize,
    #[serde(rename = "p95SpeedKmh")]
    p95_speed_kmh: Option<f64>,
    #[serde(rename = "meanHeadwaySeconds")]
    mean_headway_seconds: Option<f64>,
    #[serde(rename = "minHeadwaySeconds")]
    min_headway_seconds: Option<f64>,
    #[serde(rename = "bunchingIncidents")]
    bunching_incidents: usize,
    /// Share of vehicles running within ON_TIME_SECONDS of schedule.
    #[serde(rename = "onTimePercent")]
    on_time_percent: Option<f64>,
    /// Signed median schedule deviation: negative = the typical tram is late.
    #[serde(rename = "medianDelaySeconds")]
    median_delay_seconds: Option<f64>,
    /// Worst lateness on the line, reported as a POSITIVE number of seconds so
    /// a reader never has to remember HFP's sign convention.
    #[serde(rename = "worstLateSeconds")]
    worst_late_seconds: Option<f64>,
    /// Share of vehicles that are not moving — a congestion signal, and the
    /// reason a line's median speed can sit near zero.
    #[serde(rename = "stoppedPercent")]
    stopped_percent: Option<f64>,
}

/// Within a minute of schedule counts as on time. HSL and most European
/// operators use a threshold in this range for trams.
const ON_TIME_SECONDS: f64 = 60.0;
/// Below this a vehicle is treated as stationary. Speeds reach the module in
/// km/h (the pipeline converts from HFP's m/s), so this is 0.5 m/s.
const STOPPED_KMH: f64 = 1.8;

/// Bunching: two buses arriving at the same stop within this gap are "bunched",
/// the classic headway-regularity failure on a frequent urban line.
const BUNCHING_THRESHOLD_SECONDS: f64 = 120.0;

/// Nearest-rank percentile (no interpolation): the smallest value at or above
/// which `pct` of samples fall. Deterministic and dependency-free, which is what
/// a KPI that ends up in a funding report needs.
fn percentile(sorted: &[f64], pct: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (pct / 100.0 * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

fn compute(samples: Vec<Sample>) -> Vec<Kpi> {
    let mut by_line: BTreeMap<String, Vec<Sample>> = BTreeMap::new();
    for s in samples {
        by_line.entry(s.line.clone()).or_default().push(s);
    }

    by_line
        .into_iter()
        .map(|(line, rows)| {
            let mut speeds: Vec<f64> = rows.iter().filter_map(|r| r.speed).collect();
            speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            // Headway is the gap between consecutive arrivals AT THE SAME STOP.
            // Pooling every stop on the line would interleave unrelated arrivals
            // and manufacture near-zero gaps, so group by stop first.
            let mut by_stop: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
            for r in &rows {
                if let (Some(stop), Some(t)) = (r.at_stop.as_deref(), r.observed) {
                    by_stop.entry(stop).or_default().push(t);
                }
            }
            let mut gaps: Vec<f64> = Vec::new();
            for (_stop, mut times) in by_stop {
                times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                gaps.extend(times.windows(2).map(|w| w[1] - w[0]));
            }
            let mean_headway = if gaps.is_empty() {
                None
            } else {
                Some(gaps.iter().sum::<f64>() / gaps.len() as f64)
            };
            let min_headway = gaps.iter().copied().fold(None, |acc: Option<f64>, g| {
                Some(acc.map_or(g, |a| a.min(g)))
            });

            // Punctuality, over the vehicles that reported a schedule deviation.
            let mut delays: Vec<f64> = rows.iter().filter_map(|r| r.delay).collect();
            delays.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let on_time = if delays.is_empty() {
                None
            } else {
                let ok = delays.iter().filter(|d| d.abs() <= ON_TIME_SECONDS).count();
                Some(100.0 * ok as f64 / delays.len() as f64)
            };
            let median_delay = percentile(&delays, 50.0);
            // delays is ascending, so the first element is the most negative,
            // i.e. the latest vehicle. Report it as positive seconds late.
            let worst_late = delays.first().filter(|d| **d < 0.0).map(|d| -d);

            // `speeds` is the same set, already sorted; a count of the ones
            // under the threshold does not care about the order.
            let stopped_pct = if speeds.is_empty() {
                None
            } else {
                let n = speeds.iter().filter(|s| **s < STOPPED_KMH).count();
                Some(100.0 * n as f64 / speeds.len() as f64)
            };

            Kpi {
                vehicle_count: speeds.len(),
                arrival_count: rows.iter().filter(|r| r.at_stop.is_some()).count(),
                on_time_percent: on_time,
                median_delay_seconds: median_delay,
                worst_late_seconds: worst_late,
                stopped_percent: stopped_pct,
                p95_speed_kmh: percentile(&speeds, 95.0),
                mean_headway_seconds: mean_headway,
                min_headway_seconds: min_headway,
                bunching_incidents: gaps
                    .iter()
                    .filter(|g| **g < BUNCHING_THRESHOLD_SECONDS)
                    .count(),
                line,
            }
        })
        .collect()
}

#[no_mangle]
pub extern "C" fn process() {
    let packed = unsafe { v0_msg_as_bytes() };
    let ptr = (packed >> 32) as u32;
    let len = packed as u32;

    let input: &[u8] = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }
    };

    // A parse failure must not abort the pipeline: emit a JSON error object so
    // the surrounding Bento config can route it like any other bad message.
    let out = match serde_json::from_slice::<Vec<Sample>>(input) {
        Ok(samples) => serde_json::to_vec(&compute(samples))
            .unwrap_or_else(|e| format!(r#"{{"error":"encode: {e}"}}"#).into_bytes()),
        Err(e) => format!(r#"{{"error":"decode: {e}"}}"#).into_bytes(),
    };

    // Hand the buffer to the host and forget it here; the host frees it via
    // `deallocate` once it has copied the bytes out.
    let size = out.len() as u32;
    let dst = allocate(size);
    if dst == 0 {
        // `alloc` answers with null when the module's memory cannot grow. The
        // host reads whatever the last `v0_msg_set_bytes` named, so writing
        // through the null pointer would be the one way to turn an allocation
        // failure into a corrupted message.
        return;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(out.as_ptr(), dst as *mut u8, out.len());
        v0_msg_set_bytes(dst, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(line: &str, speed: f64, observed: f64, at_stop: Option<&str>) -> Sample {
        Sample {
            line: line.into(),
            speed: Some(speed),
            observed: Some(observed),
            at_stop: at_stop.map(str::to_string),
            delay: None,
        }
    }

    /// A vehicle sample carrying a schedule deviation, no stop involvement.
    fn d(line: &str, speed: f64, delay: f64) -> Sample {
        Sample {
            line: line.into(),
            speed: Some(speed),
            observed: None,
            at_stop: None,
            delay: Some(delay),
        }
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let v = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&v, 95.0), Some(10.0));
        assert_eq!(percentile(&v, 50.0), Some(5.0));
        assert_eq!(percentile(&[], 95.0), None);
    }

    #[test]
    fn headway_and_bunching_are_order_dependent() {
        // Arrivals at 0s, 60s, 600s -> gaps 60 and 540. One gap is under the
        // 120 s threshold, so exactly one bunching incident.
        let out = compute(vec![
            s("14", 20.0, 600.0, Some("stopA")),
            s("14", 30.0, 0.0, Some("stopA")),
            s("14", 25.0, 60.0, Some("stopA")),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bunching_incidents, 1);
        assert_eq!(out[0].min_headway_seconds, Some(60.0));
        assert_eq!(out[0].mean_headway_seconds, Some(300.0));
    }

    #[test]
    fn vehicles_not_at_a_stop_are_excluded_from_headway() {
        let out = compute(vec![s("5", 10.0, 0.0, None), s("5", 10.0, 30.0, None)]);
        assert_eq!(out[0].vehicle_count, 2);
        assert_eq!(out[0].arrival_count, 0, "no at_stop rows here");
        assert_eq!(out[0].mean_headway_seconds, None);
        assert_eq!(out[0].bunching_incidents, 0);
    }

    #[test]
    fn headway_is_per_stop_not_pooled_across_the_line() {
        // Two stops, each with arrivals 600 s apart. Pooling them would sort to
        // 0,10,600,610 and report gaps of 10 s -> false bunching.
        let out = compute(vec![
            s("9", 20.0, 0.0, Some("stopA")),
            s("9", 20.0, 10.0, Some("stopB")),
            s("9", 20.0, 600.0, Some("stopA")),
            s("9", 20.0, 610.0, Some("stopB")),
        ]);
        assert_eq!(out[0].mean_headway_seconds, Some(600.0));
        assert_eq!(out[0].bunching_incidents, 0, "pooled stops would report 2");
    }

    #[test]
    fn punctuality_counts_both_directions_of_lateness() {
        // -120 late, -10 on time, +30 on time, +200 early -> 2 of 4 on time.
        let out = compute(vec![
            d("4", 20.0, -120.0),
            d("4", 20.0, -10.0),
            d("4", 20.0, 30.0),
            d("4", 20.0, 200.0),
        ]);
        assert_eq!(out[0].on_time_percent, Some(50.0));
        // Worst lateness is reported POSITIVE even though HFP's sign is negative.
        assert_eq!(out[0].worst_late_seconds, Some(120.0));
        assert_eq!(
            out[0].median_delay_seconds,
            Some(-10.0),
            "nearest-rank median"
        );
    }

    #[test]
    fn a_line_with_no_late_vehicle_reports_no_worst_lateness() {
        let out = compute(vec![d("7", 20.0, 15.0), d("7", 20.0, 40.0)]);
        assert_eq!(out[0].on_time_percent, Some(100.0));
        assert_eq!(out[0].worst_late_seconds, None, "nothing was late");
    }

    #[test]
    fn stopped_percent_uses_the_kmh_threshold() {
        // 1.0 and 0.0 km/h are stationary; 5.0 and 30.0 are moving.
        let out = compute(vec![
            d("9", 0.0, 0.0),
            d("9", 1.0, 0.0),
            d("9", 5.0, 0.0),
            d("9", 30.0, 0.0),
        ]);
        assert_eq!(out[0].stopped_percent, Some(50.0));
    }

    #[test]
    fn punctuality_is_absent_when_no_vehicle_reported_a_delay() {
        let out = compute(vec![s("3", 20.0, 0.0, Some("stopA"))]);
        assert_eq!(out[0].on_time_percent, None);
        assert_eq!(out[0].median_delay_seconds, None);
    }

    /// A line seen only through arrivals has no speed anywhere, and every
    /// KPI derived from speed has to be absent rather than zero: an empty
    /// sample set means "not measured", and a p95 of 0 or "100% standing
    /// still" on a moving tram line would be read as a finding.
    #[test]
    fn a_line_known_only_from_arrivals_reports_no_speed_kpi() {
        let arrival = |line: &str, at: f64| Sample {
            line: line.into(),
            speed: None,
            observed: Some(at),
            at_stop: Some("stopA".into()),
            delay: None,
        };
        let out = compute(vec![arrival("6", 0.0), arrival("6", 300.0)]);
        assert_eq!(out[0].vehicle_count, 0, "no sample carried a speed");
        assert_eq!(out[0].arrival_count, 2);
        assert_eq!(out[0].p95_speed_kmh, None);
        assert_eq!(out[0].stopped_percent, None, "unmeasured is not stopped");
        assert_eq!(out[0].mean_headway_seconds, Some(300.0));
    }

    #[test]
    fn lines_are_aggregated_separately() {
        let out = compute(vec![
            s("1", 50.0, 0.0, Some("x")),
            s("2", 10.0, 0.0, Some("x")),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].line, "1");
        assert_eq!(out[1].line, "2");
    }
}
