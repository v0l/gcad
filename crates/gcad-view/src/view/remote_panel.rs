use crate::host::Instant;
use crate::remote::{self, Limits, Overview};
use egui::Ui;
use egui_bench::prelude::*;
use gcad_sims_proto::{self as proto, Job, JobState};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Price {
    Any,
    Free,
    Cap,
}

type Action = (Receiver<Result<String, String>>, &'static str);

pub struct RemotePanel {
    pub open: bool,
    overview: Option<Overview>,
    fetching: Option<Receiver<Overview>>,
    last: Option<Instant>,
    minutes: u64,
    price: Price,
    cap_euros: f64,
    action: Option<Action>,
    message: Option<(String, bool)>,
    has_account: bool,
}

impl RemotePanel {
    pub fn new() -> Self {
        RemotePanel {
            open: false,
            overview: None,
            fetching: None,
            last: None,
            minutes: 60,
            price: Price::Any,
            cap_euros: 2.5,
            action: None,
            message: None,
            has_account: remote::has_account(),
        }
    }

    pub fn limits(&self) -> Limits {
        Limits {
            minutes: self.minutes,
            price_cents: match self.price {
                Price::Any => None,
                Price::Free => Some(0),
                Price::Cap => Some((self.cap_euros * 100.0).round().max(0.0) as u64),
            },
        }
    }

    pub fn refresh(&mut self) {
        self.last = None;
    }

    pub fn balance(&self) -> Option<i64> {
        self.overview
            .as_ref()
            .and_then(|o| o.account.as_ref())
            .map(|a| a.balance_cents)
    }

    pub fn active_jobs(&self) -> usize {
        self.overview.as_ref().map_or(0, |o| o.active().count())
    }

    pub fn tick(&mut self, ctx: &egui::Context, busy: bool) {
        if let Some(rx) = &self.fetching {
            match rx.try_recv() {
                Ok(view) => {
                    self.has_account = view.signed_in;
                    self.overview = Some(view);
                    self.fetching = None;
                    self.last = Some(Instant::now());
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(150)),
                Err(TryRecvError::Disconnected) => self.fetching = None,
            }
        }
        if let Some((rx, _)) = &self.action {
            match rx.try_recv() {
                Ok(result) => {
                    self.message = Some(match result {
                        Ok(text) => (text, false),
                        Err(text) => (text, true),
                    });
                    self.action = None;
                    self.refresh();
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(150)),
                Err(TryRecvError::Disconnected) => self.action = None,
            }
        }
        let first = self.overview.is_none() && self.has_account;
        if !(self.open || busy || first) || self.fetching.is_some() {
            return;
        }
        let every = if busy || self.active_jobs() > 0 {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(30)
        };
        let waited = self.last.map(|t| t.elapsed());
        match waited {
            Some(w) if w < every => ctx.request_repaint_after(every - w),
            _ => {
                let (tx, rx) = channel();
                let ctx = ctx.clone();
                crate::host::spawn(move || {
                    let _ = tx.send(remote::overview());
                    crate::host::wake(&ctx);
                });
                self.fetching = Some(rx);
            }
        }
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        what: &'static str,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if self.action.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        crate::host::spawn(move || {
            let _ = tx.send(work());
            crate::host::wake(&ctx);
        });
        self.message = None;
        self.action = Some((rx, what));
    }

    pub fn summary(&self) -> String {
        let mut out = String::from("server");
        if let Some(balance) = self.balance() {
            out.push(' ');
            out.push_str(&remote::euros(balance));
        }
        let active = self.active_jobs();
        if active > 0 {
            out.push_str(&format!(", {active} running"));
        }
        out
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let Some(view) = self.overview.clone() else {
            hint(
                ui,
                if self.fetching.is_some() {
                    "asking the sims server"
                } else {
                    "not asked yet"
                },
            );
            return;
        };
        Line::new()
            .legend("server")
            .value(host_of(&view.server))
            .size(11.0)
            .show(ui);
        if let Some(error) = &view.error {
            note(ui, error, FAULT);
        }
        self.runners(ui, &view);
        match (&view.account, view.signed_in) {
            (Some(account), _) => self.account(ui, &view, account),
            (None, true) => {}
            (None, false) => {
                hint(
                    ui,
                    "no account on this machine. making one is free, runs are prepaid",
                );
                if toggle(ui, "buy credit", false)
                    .on_hover_text(view.shop())
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(view.shop()));
                }
                let busy = self.action.is_some();
                if !busy && toggle(ui, "make account", false).clicked() {
                    self.start(ui.ctx(), "making an account", || {
                        remote::create_account(None)
                            .map(|(made, server, _)| {
                                format!(
                                    "account {} made on {server}, top it up with that id",
                                    made.id
                                )
                            })
                            .map_err(|e| format!("{e:#}"))
                    });
                }
            }
        }
        if let Some((_, what)) = &self.action {
            lamp(ui, what, true, false);
        }
        if let Some((text, fault)) = &self.message {
            note(ui, text, if *fault { FAULT } else { OK });
        }
        if view.signed_in {
            self.jobs(ui, &view);
        }
    }

    fn runners(&self, ui: &mut Ui, view: &Overview) {
        let online = view.online_on_this_build();
        if online.is_empty() {
            let text = if view.known_on_this_build() > 0 {
                "runners for this version are offline".to_string()
            } else if view.versions.is_empty() {
                format!(
                    "no runner runs gcad {}",
                    proto::line_label(Overview::build())
                )
            } else {
                format!(
                    "no runner runs gcad {}, they run {}",
                    proto::line_label(Overview::build()),
                    view.versions.join(", ")
                )
            };
            note(ui, text, WARN);
            return;
        }
        let cheapest = view.cheapest().unwrap_or(0);
        Line::new()
            .legend("runners")
            .value(format!("{} compatible", online.len()))
            .note(format!(
                "from {}, {} queued",
                proto::price_label(cheapest),
                view.queued
            ))
            .size(11.0)
            .show(ui);
        for runner in online.iter().take(4) {
            let info = &runner.info;
            Line::new()
                .legend(&runner.name)
                .value(info.gpu.as_deref().unwrap_or("no GPU"))
                .note(format!(
                    "{}{}, {}",
                    info.tflops
                        .map_or(String::new(), |t| format!("{t:.1} TFLOPS, ")),
                    info.vram_gb.map_or(String::new(), |v| format!("{v:.0} GB")),
                    if runner.busy {
                        format!("busy, {}", proto::price_label(runner.price_cents_per_hour))
                    } else {
                        format!("idle, {}", proto::price_label(runner.price_cents_per_hour))
                    }
                ))
                .size(11.0)
                .elided(ui);
        }
    }

    fn account(&mut self, ui: &mut Ui, view: &Overview, account: &proto::Account) {
        Line::new()
            .legend("account")
            .value(&account.id)
            .size(11.0)
            .show(ui);
        let hold = remote::hold_cents(
            self.minutes,
            match self.limits().price_cents {
                Some(cap) => cap,
                None => view.priciest().unwrap_or(0).max(0) as u64,
            },
        );
        let short = account.balance_cents < hold;
        Line::new()
            .legend("balance")
            .value(remote::euros(account.balance_cents))
            .note(if account.held_cents > 0 {
                format!("{} held by running jobs", remote::euros(account.held_cents))
            } else {
                String::new()
            })
            .tint(if short { WARN } else { READOUT })
            .show(ui);
        ui.horizontal(|ui| {
            if toggle(ui, "buy credit", false)
                .on_hover_text(view.shop())
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(view.shop()));
            }
            if toggle(ui, "copy id", false).clicked() {
                ui.ctx().copy_text(account.id.clone());
            }
        });
        ui.horizontal(|ui| {
            Line::new().legend("longest").show(ui);
            ui.add(
                egui::DragValue::new(&mut self.minutes)
                    .range(1..=1440)
                    .suffix(" min"),
            );
        });
        ui.horizontal(|ui| {
            Line::new().legend("price").show(ui);
            for (choice, label) in [
                (Price::Any, "any"),
                (Price::Free, "free"),
                (Price::Cap, "up to"),
            ] {
                if toggle(ui, label, self.price == choice).clicked() {
                    self.price = choice;
                }
            }
            if self.price == Price::Cap {
                ui.add(
                    egui::DragValue::new(&mut self.cap_euros)
                        .range(0.0..=1000.0)
                        .speed(0.05)
                        .prefix("€")
                        .suffix("/h"),
                );
            }
        });
        if self.price != Price::Free {
            note(
                ui,
                if short {
                    format!(
                        "a run holds up to {}, more than the balance. buy credit or lower the limits",
                        remote::euros(hold)
                    )
                } else {
                    format!(
                        "a run holds up to {} until it finishes",
                        remote::euros(hold)
                    )
                },
                if short { WARN } else { LEGEND },
            );
        }
    }

    fn jobs(&mut self, ui: &mut Ui, view: &Overview) {
        if view.jobs.is_empty() {
            hint(ui, "no runs on the server yet");
            return;
        }
        ui.add_space(4.0);
        Line::new().legend("recent runs").size(11.0).show(ui);
        for job in view.jobs.iter().take(6) {
            let tint = match job.state {
                JobState::Succeeded => OK,
                JobState::Failed => FAULT,
                JobState::Cancelled => LEGEND,
                _ => TRACE,
            };
            Line::new()
                .legend(job.state.name())
                .value(&job.spec)
                .note(job_note(job))
                .tint(tint)
                .size(11.0)
                .elided(ui);
            if job.state.active() {
                if let Some(progress) = &job.progress {
                    note(ui, progress, LEGEND);
                }
                if job.state != JobState::Cancelling
                    && self.action.is_none()
                    && toggle(ui, &format!("cancel {}", short_id(&job.id)), false).clicked()
                {
                    let id = job.id.clone();
                    self.start(ui.ctx(), "cancelling", move || {
                        remote::cancel_job(&id)
                            .map(|j| format!("{} {}", short_id(&id), j.state.name()))
                            .map_err(|e| format!("{e:#}"))
                    });
                }
            } else if let Some(error) = &job.error {
                note(ui, error, FAULT);
            }
        }
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn host_of(server: &str) -> String {
    server
        .split_once("://")
        .map_or(server, |(_, rest)| rest)
        .trim_end_matches('/')
        .to_string()
}

fn job_note(job: &Job) -> String {
    let mut parts = Vec::new();
    match job.state {
        JobState::Queued => parts.push(format!("{} ahead", job.queue_position.unwrap_or(0))),
        _ => {
            if let Some(runner) = &job.runner {
                parts.push(runner.clone());
            }
            if let Some(seconds) = job.seconds {
                parts.push(remote::duration(seconds));
            }
        }
    }
    match (job.charged_cents, job.state.active()) {
        (Some(charged), _) => parts.push(remote::euros(charged)),
        (None, true) if job.held_cents > 0 => {
            parts.push(format!("{} held", remote::euros(job.held_cents)))
        }
        _ => {}
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(state: JobState) -> Job {
        Job {
            id: "abcdef0123456789".into(),
            spec: "wind.gsim".into(),
            vars: Vec::new(),
            version: "0.4.0".into(),
            hash: String::new(),
            state,
            progress: None,
            error: None,
            files: Vec::new(),
            max_seconds: 3600,
            price_cents_per_hour: 250,
            max_price_cents_per_hour: 250,
            runner: Some("rtx".into()),
            held_cents: 250,
            charged_cents: None,
            seconds: Some(95.0),
            queue_position: Some(2),
            created_at: 1,
            started_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn limits_follow_the_price_choice() {
        let mut panel = RemotePanel::new();
        assert_eq!(panel.limits().price_cents, None);
        assert_eq!(panel.limits().minutes, 60);
        panel.price = Price::Free;
        assert_eq!(panel.limits().price_cents, Some(0));
        panel.price = Price::Cap;
        panel.cap_euros = 2.5;
        panel.minutes = 20;
        let limits = panel.limits();
        assert_eq!((limits.minutes, limits.price_cents), (20, Some(250)));
    }

    #[test]
    fn job_notes_say_where_a_run_is_and_what_it_costs() {
        assert_eq!(job_note(&job(JobState::Queued)), "2 ahead, €2.50 held");
        assert_eq!(job_note(&job(JobState::Running)), "rtx, 1m35s, €2.50 held");
        let mut done = job(JobState::Succeeded);
        done.charged_cents = Some(7);
        assert_eq!(job_note(&done), "rtx, 1m35s, €0.07");
        assert_eq!(short_id("abcdef0123456789"), "abcdef01");
        assert_eq!(host_of("https://sims.gcad.sh/"), "sims.gcad.sh");
    }

    #[test]
    fn the_summary_shows_balance_and_running_jobs() {
        let mut panel = RemotePanel::new();
        assert_eq!(panel.summary(), "server");
        let mut view = Overview::default();
        view.signed_in = true;
        view.account = Some(proto::Account {
            id: "gsa_x".into(),
            balance_cents: 1234,
            held_cents: 0,
            shop_url: None,
        });
        view.jobs = vec![job(JobState::Running), job(JobState::Succeeded)];
        panel.overview = Some(view);
        assert_eq!(panel.summary(), "server €12.34, 1 running");
    }
}
