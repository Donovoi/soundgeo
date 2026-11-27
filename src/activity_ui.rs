//! Visible lifecycle for a worker, including stages that have not run yet.
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Waiting,
    Running,
    Complete,
    Attention,
    Stopped,
    NotRun,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Waiting => "Waiting",
            Self::Running => "Working",
            Self::Complete => "Complete",
            Self::Attention => "Needs attention",
            Self::Stopped => "Stopped",
            Self::NotRun => "Not run",
        }
    }
}

pub(super) struct Activity {
    pub title: String,
    pub started: Instant,
    pub updated: Instant,
    pub finished: Option<Instant>,
    pub stages: Vec<(&'static str, State)>,
}
impl Activity {
    pub fn new(title: &str) -> Self {
        let now = Instant::now();
        Self {
            title: title.into(),
            started: now,
            updated: now,
            finished: None,
            stages: vec![],
        }
    }
    pub fn plan(&mut self, stages: impl IntoIterator<Item = &'static str>) {
        self.stages = stages
            .into_iter()
            .map(|name| (name, State::Waiting))
            .collect();
    }
    pub fn stage(&mut self, name: &'static str, state: State) {
        if self.finished.is_some() {
            return;
        }
        if let Some(entry) = self.stages.iter_mut().find(|(label, _)| *label == name) {
            entry.1 = state;
        }
        self.updated = Instant::now();
    }
    pub fn finish(&mut self, stopped: bool) {
        if self.finished.is_some() {
            return;
        }
        self.finished = Some(Instant::now());
        for (_, state) in &mut self.stages {
            *state = match *state {
                State::Waiting => State::NotRun,
                State::Running if stopped => State::Stopped,
                State::Running => State::Attention,
                other => other,
            };
        }
    }
    pub fn elapsed(&self) -> u64 {
        self.finished
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
            .as_secs()
    }
}

pub(super) fn duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_keeps_finished_clues_and_does_not_claim_waiting_work_ran() {
        let mut task = Activity::new("Analyze recording");
        task.plan(["Video clues", "Speech", "Location search"]);
        task.stage("Video clues", State::Complete);
        task.stage("Speech", State::Running);
        task.finish(true);
        assert_eq!(
            task.stages,
            [
                ("Video clues", State::Complete),
                ("Speech", State::Stopped),
                ("Location search", State::NotRun)
            ]
        );
        let ended = task.finished;
        task.stage("Speech", State::Complete);
        task.finish(false);
        assert_eq!(task.finished, ended);
        assert_eq!(task.stages[1].1, State::Stopped);
    }

    #[test]
    fn optional_problem_remains_visible_after_location_finishes() {
        let mut task = Activity::new("Analyze recording");
        task.plan(["Speech", "Location search"]);
        task.stage("Speech", State::Attention);
        task.stage("Location search", State::Running);
        task.stage("Location search", State::Complete);
        task.finish(false);
        assert_eq!(
            task.stages,
            [
                ("Speech", State::Attention),
                ("Location search", State::Complete)
            ]
        );
    }
}
