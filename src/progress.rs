use crate::host::Instant;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub phase: Option<String>,
    pub stage: String,
    pub done: f64,
    pub total: Option<f64>,
    pub unit: &'static str,
    pub fraction: Option<f64>,
    pub detail: String,
    pub elapsed: Duration,
    pub finished: bool,
}

impl Report {
    pub fn fraction(&self) -> Option<f64> {
        self.fraction
            .or_else(|| self.total.filter(|t| *t > 0.0).map(|t| self.done / t))
            .map(|f| f.clamp(0.0, 1.0))
    }

    pub fn remaining(&self) -> Option<Duration> {
        let fraction = self.fraction()?;
        let elapsed = self.elapsed.as_secs_f64();
        let left = elapsed * (1.0 - fraction) / fraction;
        (fraction > 0.02 && elapsed > 1.0 && left >= 1.0 && !self.finished)
            .then(|| Duration::from_secs_f64(left))
    }

    pub fn name(&self) -> String {
        match &self.phase {
            Some(phase) => format!("{phase}: {}", self.stage),
            None => self.stage.clone(),
        }
    }

    pub fn status(&self) -> String {
        let mut parts = Vec::new();
        let count = match self.total {
            Some(total) => format!("{} of {} {}", whole(self.done), whole(total), self.unit),
            None if self.done > 0.0 => format!("{} {}", whole(self.done), self.unit),
            None => String::new(),
        };
        if !count.trim().is_empty() {
            parts.push(count.trim().to_string());
        }
        if let Some(fraction) = self.fraction()
            && !self.finished
        {
            parts.push(format!("{:.0}%", fraction * 100.0));
        }
        if !self.detail.is_empty() {
            parts.push(self.detail.clone());
        }
        if self.finished {
            parts.push(format!("took {}", duration(self.elapsed)));
        } else {
            parts.push(format!("{} so far", duration(self.elapsed)));
            if let Some(left) = self.remaining() {
                parts.push(format!("about {} left", duration(left)));
            }
        }
        parts.join(", ")
    }

    pub fn line(&self) -> String {
        format!("{}, {}", self.name(), self.status())
    }
}

fn whole(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

pub fn duration(span: Duration) -> String {
    let seconds = span.as_secs_f64();
    if seconds < 10.0 {
        format!("{seconds:.1} s")
    } else if seconds < 90.0 {
        format!("{seconds:.0} s")
    } else if seconds < 5400.0 {
        format!("{}m{:02}s", seconds as u64 / 60, seconds as u64 % 60)
    } else {
        format!("{}h{:02}m", seconds as u64 / 3600, seconds as u64 / 60 % 60)
    }
}

type Sink = Arc<dyn Fn(&Report) + Send + Sync>;

static SINK: RwLock<Option<Sink>> = RwLock::new(None);
static PHASE: Mutex<Vec<String>> = Mutex::new(Vec::new());

const EVERY: Duration = Duration::from_millis(100);

pub struct Listening(());

impl Drop for Listening {
    fn drop(&mut self) {
        *SINK.write().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub fn listen(sink: impl Fn(&Report) + Send + Sync + 'static) -> Listening {
    *SINK.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(sink));
    Listening(())
}

fn sink() -> Option<Sink> {
    SINK.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub struct Phase(bool);

impl Drop for Phase {
    fn drop(&mut self) {
        if self.0 {
            PHASE.lock().unwrap_or_else(|e| e.into_inner()).pop();
        }
    }
}

pub fn phase(name: impl Into<String>) -> Phase {
    if sink().is_none() {
        return Phase(false);
    }
    PHASE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(name.into());
    Phase(true)
}

fn current_phase() -> Option<String> {
    let phases = PHASE.lock().unwrap_or_else(|e| e.into_inner());
    (!phases.is_empty()).then(|| phases.join(", "))
}

struct State {
    announced: bool,
    done: f64,
    fraction: Option<f64>,
    detail: String,
    sent: Instant,
}

struct Inner {
    sink: Sink,
    phase: Option<String>,
    name: String,
    total: Option<f64>,
    unit: &'static str,
    started: Instant,
    delay: Duration,
    state: Mutex<State>,
}

pub struct Stage(Option<Inner>);

pub fn stage(name: impl Into<String>, total: Option<usize>, unit: &'static str) -> Stage {
    stage_after(name, total, unit, Duration::ZERO)
}

pub fn stage_after(
    name: impl Into<String>,
    total: Option<usize>,
    unit: &'static str,
    delay: Duration,
) -> Stage {
    let Some(sink) = sink() else {
        return Stage(None);
    };
    let stage = Stage(Some(Inner {
        sink,
        phase: current_phase(),
        name: name.into(),
        total: total.map(|t| t as f64),
        unit,
        started: Instant::now(),
        delay,
        state: Mutex::new(State {
            announced: delay.is_zero(),
            done: 0.0,
            fraction: None,
            detail: String::new(),
            sent: Instant::now(),
        }),
    }));
    if delay.is_zero() {
        stage.send(false);
    }
    stage
}

impl Stage {
    pub fn silent() -> Stage {
        Stage(None)
    }

    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    pub fn advance(&self, by: f64) {
        self.update(|state| state.done += by);
    }

    pub fn set(&self, done: f64) {
        self.update(|state| state.done = done);
    }

    pub fn fraction(&self, fraction: f64) {
        self.update(|state| state.fraction = Some(fraction));
    }

    pub fn detail(&self, text: impl FnOnce() -> String) {
        if self.0.is_some() {
            let text = text();
            self.update(|state| state.detail = text);
        }
    }

    pub fn progress(&self, done: f64, text: impl FnOnce() -> String) {
        if self.0.is_some() {
            let text = text();
            self.update(|state| {
                state.done = done;
                state.detail = text;
            });
        }
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        let Some(inner) = &self.0 else { return };
        let due = {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            change(&mut state);
            if !state.announced && inner.started.elapsed() >= inner.delay {
                state.announced = true;
                state.sent = inner.started;
            }
            let due = state.announced && state.sent.elapsed() >= EVERY;
            if due {
                state.sent = Instant::now();
            }
            due
        };
        if due {
            self.send(false);
        }
    }

    fn send(&self, finished: bool) {
        let Some(inner) = &self.0 else { return };
        let report = {
            let state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.announced {
                return;
            }
            Report {
                phase: inner.phase.clone(),
                stage: inner.name.clone(),
                done: state.done,
                total: inner.total,
                unit: inner.unit,
                fraction: state.fraction,
                detail: state.detail.clone(),
                elapsed: inner.started.elapsed(),
                finished,
            }
        };
        (inner.sink)(&report);
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.send(true);
    }
}

pub fn residual_fraction(start: f64, now: f64, goal: f64) -> f64 {
    if start <= goal || now <= 0.0 {
        return 1.0;
    }
    ((start / now.max(goal)).ln() / (start / goal).ln()).clamp(0.0, 1.0)
}

#[cfg(test)]
pub(crate) static SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_report_to_the_sink_and_finish() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let seen: Arc<Mutex<Vec<Report>>> = Arc::default();
        let into = seen.clone();
        let listening = listen(move |r| into.lock().unwrap().push(r.clone()));
        {
            let _phase = phase("coarse check");
            let stage = stage("meshing", Some(4), "parts");
            stage.advance(4.0);
        }
        drop(listening);
        let seen = seen.lock().unwrap();
        let last = seen
            .iter()
            .rfind(|r| r.stage == "meshing")
            .expect("a report");
        assert!(last.finished && last.done == 4.0, "{last:?}");
        assert_eq!(last.phase.as_deref(), Some("coarse check"));
        assert!(
            last.line()
                .starts_with("coarse check: meshing, 4 of 4 parts, took")
        );
        assert!(!stage("unheard", None, "").active());
    }

    #[test]
    fn quick_delayed_stages_stay_silent() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let seen: Arc<Mutex<Vec<Report>>> = Arc::default();
        let into = seen.clone();
        let listening = listen(move |r| into.lock().unwrap().push(r.clone()));
        {
            let stage = stage_after("quick part", None, "", Duration::from_secs(60));
            stage.advance(1.0);
        }
        drop(listening);
        assert!(!seen.lock().unwrap().iter().any(|r| r.stage == "quick part"));
    }

    #[test]
    fn residual_progress_is_logarithmic() {
        assert!((residual_fraction(1.0, 1.0e-5, 1.0e-10) - 0.5).abs() < 1.0e-12);
        assert_eq!(residual_fraction(1.0, 1.0e-12, 1.0e-10), 1.0);
    }
}
