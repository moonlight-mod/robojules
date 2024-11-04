use crate::logic::{
    diff::{DiffedExtension, PullRequestUpdate},
    LogicError, LogicResult,
};

#[derive(Debug)]
pub struct AsyncState<T> {
    pub value: Option<T>,
    pub working: bool,
    pub error: Option<LogicError>,
}

impl<T> Default for AsyncState<T> {
    fn default() -> Self {
        Self {
            value: None,
            working: false,
            error: None,
        }
    }
}

impl<T> AsyncState<T> {
    pub fn new(value: Option<T>) -> Self {
        Self {
            value,
            working: false,
            error: None,
        }
    }

    pub fn set(&mut self, result: LogicResult<T>) {
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.working = false;
                self.error = None;
            }
            Err(err) => {
                self.value = None;
                self.working = false;
                self.error = Some(err);
            }
        }
    }

    pub fn start(&mut self) {
        self.working = true;
    }

    pub fn clear(&mut self) {
        self.value = None;
        self.working = false;
        self.error = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewType {
    #[default]
    Source,
    Asar,
}

#[derive(Debug, Default)]
pub struct AppState {
    pub pull_request_id: u64,
    pub pull_request_update: AsyncState<PullRequestUpdate>,

    pub selected_extension: Option<String>,
    pub diffed_extension: AsyncState<DiffedExtension>,

    pub view_type: ViewType,
    pub selected_file: Option<String>,
    pub diff: Option<String>,
}
