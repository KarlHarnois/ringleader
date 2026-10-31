use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use crate::application::ports::{ProcessMeter, ProcessMeterError};
use crate::domain::ProcessSample;
use crate::test_support::CallHooks;

#[derive(Default)]
pub struct FakeProcessMeter {
    state: Mutex<MeterState>,
    hooks: CallHooks,
}

#[derive(Default)]
struct MeterState {
    samples: VecDeque<ProcessSample>,
    denial: Option<String>,
    samples_taken: usize,
}

impl FakeProcessMeter {
    const SAMPLE: &'static str = "sample";

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_samples(self, samples: Vec<ProcessSample>) -> Self {
        self.state.lock().unwrap().samples = samples.into();
        self
    }

    pub fn unreadable(self, denial: &str) -> Self {
        self.state.lock().unwrap().denial = Some(denial.to_string());
        self
    }

    pub fn on_sample(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.hooks.add(Self::SAMPLE, hook);
    }

    pub fn samples_taken(&self) -> usize {
        self.state.lock().unwrap().samples_taken
    }
}

impl ProcessMeter for FakeProcessMeter {
    fn sample(&self) -> Result<ProcessSample, ProcessMeterError> {
        self.state.lock().unwrap().samples_taken += 1;
        self.hooks.run(Self::SAMPLE);
        self.state.lock().unwrap().next_sample()
    }
}

impl MeterState {
    fn next_sample(&mut self) -> Result<ProcessSample, ProcessMeterError> {
        if let Some(detail) = &self.denial {
            return Err(ProcessMeterError::Unreadable {
                detail: detail.clone(),
            });
        }
        let sample = if self.samples.len() > 1 {
            self.samples.pop_front()
        } else {
            self.samples.front().cloned()
        };
        Ok(sample.unwrap_or_else(|| ProcessSample::new(Duration::ZERO, Vec::new())))
    }
}
