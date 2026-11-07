use egui_bench::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    pub time: f64,
    pub playing: bool,
    pub looping: bool,
    pub speed: f64,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            time: 0.0,
            playing: false,
            looping: true,
            speed: 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Skin {
    Card,
    Bar,
}

impl Timeline {
    pub fn advance(&mut self, duration: f64, seconds: f64) {
        if !self.playing {
            return;
        }
        if duration <= 0.0 {
            self.playing = false;
            return;
        }
        self.time += seconds * self.speed;
        if self.looping {
            self.time = self.time.rem_euclid(duration);
        } else if self.time >= duration {
            self.time = duration;
            self.playing = false;
        } else if self.time < 0.0 {
            self.time = 0.0;
            self.playing = false;
        }
    }

    pub fn rewind(&mut self) {
        self.time = 0.0;
        self.playing = false;
    }

    pub fn toggle_playing(&mut self, duration: f64) {
        if !self.playing && !self.looping && self.time >= duration {
            self.time = 0.0;
        }
        self.playing = !self.playing;
    }

    pub fn step(&mut self, duration: f64, seconds: f64) {
        self.playing = false;
        self.time = (self.time + seconds).clamp(0.0, duration);
    }

    pub fn clamp(&mut self, duration: f64) {
        self.time = self.time.clamp(0.0, duration.max(0.0));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, duration: f64, fps: Option<f64>, skin: Skin) {
        match skin {
            Skin::Card => self.card(ui, duration, fps),
            Skin::Bar => self.bar(ui, duration),
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, duration: f64, fps: Option<f64>) {
        let duration = duration.max(1.0e-6);
        let fps = fps.unwrap_or(30.0);
        ui.horizontal(|ui| {
            let label = if self.playing { "pause" } else { "play" };
            if toggle(ui, label, self.playing).clicked() {
                self.toggle_playing(duration);
            }
            if toggle(ui, "loop", self.looping).clicked() {
                self.looping = !self.looping;
            }
            let frame = 1.0 / fps.max(1.0);
            if toggle(ui, "back", false).clicked() {
                self.step(duration, -frame);
            }
            if toggle(ui, "next", false).clicked() {
                self.step(duration, frame);
            }
        });
        ui.spacing_mut().slider_width = ui.available_width() - 10.0;
        if ui
            .add(egui::Slider::new(&mut self.time, 0.0..=duration).show_value(false))
            .on_hover_text("time")
            .changed()
        {
            self.playing = false;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 110.0;
            ui.add(
                egui::Slider::new(&mut self.speed, 0.1..=4.0)
                    .logarithmic(true)
                    .show_value(false),
            )
            .on_hover_text("playback speed");
            Line::new()
                .measured(format!("{:.2}x", self.speed))
                .size(11.0)
                .show(ui);
        });
        Line::new()
            .legend("time")
            .measured(format!("{:.3} / {:.3} s", self.time, duration))
            .legend("frame")
            .measured(format!(
                "{} at {} fps",
                (self.time * fps).round() as i64,
                fps
            ))
            .size(11.0)
            .show(ui);
    }

    fn bar(&mut self, ui: &mut egui::Ui, duration: f64) {
        if ui
            .add_enabled(
                duration > 0.,
                egui::Button::new(if self.playing { "Pause" } else { "Play" }),
            )
            .clicked()
        {
            self.toggle_playing(duration);
        }
        ui.checkbox(&mut self.looping, "Loop");
        ui.add(
            egui::Slider::new(&mut self.time, 0.0..=duration)
                .fixed_decimals(2)
                .text("Seconds"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_loops_steps_and_stops_at_the_end() {
        let mut t = Timeline {
            playing: true,
            ..Default::default()
        };
        t.advance(2.0, 1.5);
        t.advance(2.0, 1.0);
        assert!((t.time - 0.5).abs() < 1e-9);
        t.looping = false;
        t.advance(2.0, 5.0);
        assert_eq!((t.time, t.playing), (2.0, false));
        t.toggle_playing(2.0);
        assert_eq!((t.time, t.playing), (0.0, true));
        t.step(2.0, -1.0);
        assert_eq!((t.time, t.playing), (0.0, false));
    }

    #[test]
    fn speed_scales_advance_and_negative_wraps() {
        let mut t = Timeline {
            playing: true,
            speed: 2.0,
            ..Default::default()
        };
        t.advance(10.0, 1.0);
        assert!((t.time - 2.0).abs() < 1e-9);
        t.speed = 0.5;
        t.advance(10.0, 1.0);
        assert!((t.time - 2.5).abs() < 1e-9);
        t.speed = -1.0;
        t.advance(10.0, 3.0);
        assert!((t.time - 9.5).abs() < 1e-9);
    }

    #[test]
    fn step_moves_by_one_frame_and_clamps() {
        let mut t = Timeline {
            playing: true,
            ..Default::default()
        };
        t.step(1.0, 0.25);
        assert_eq!((t.time, t.playing), (0.25, false));
        t.step(1.0, 5.0);
        assert_eq!(t.time, 1.0);
    }

    #[test]
    fn bar_shows_seconds_with_two_decimals() {
        use egui_kittest::kittest::Queryable as _;
        let mut harness = egui_kittest::Harness::builder().build_ui_state(
            |ui, t: &mut Timeline| t.show(ui, 2.0, None, Skin::Bar),
            Timeline::default(),
        );
        harness.run();
        let value = harness.get_by_role(egui::accesskit::Role::SpinButton).value();
        assert_eq!(value.as_deref(), Some("0.00"));
    }

    #[test]
    fn zero_duration_stops_playing() {
        let mut t = Timeline {
            playing: true,
            ..Default::default()
        };
        t.advance(0.0, 1.0);
        assert!(!t.playing);
    }
}
