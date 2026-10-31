use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use crate::application::ports::{ProcessMeter, ProcessMeterError};
use crate::domain::ProcessSample;

#[derive(Default)]
pub struct FakeProcessMeter {
    state: Mutex<MeterState>,
}

#[derive(Default)]
struct MeterState {
    samples: VecDeque<ProcessSample>,
    denial: Option<String>,
}

impl FakeProcessMeter {
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
}

impl ProcessMeter for FakeProcessMeter {
    fn sample(&self) -> Result<ProcessSample, ProcessMeterError> {
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
