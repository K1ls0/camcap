use std::time::Duration;


#[derive(Debug, Clone, Copy)]
pub enum TimerMsg {
    CaptureImage
}


#[derive(Debug, Clone, Copy)]
pub enum SetTimerMsg {
    Interval(Option<Duration>)
}
