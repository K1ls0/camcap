use std::sync::{Arc, RwLock};

use app::State;
use timer::SetTimerMsg;
use tokio::{sync::mpsc, time::Interval};

use crate::timer::TimerMsg;

mod app;
mod timer;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _eg = rt.enter();

    let (timer_sender, timer_receiver) = mpsc::unbounded_channel();
    let (set_timer_sender, mut set_timer_receiver) = mpsc::unbounded_channel();

    let _timing_thread = std::thread::spawn(move || {
        rt.block_on(async move {
            let mut interval: Option<Interval> = None;
            loop {
                if let Some(ref mut cinterval) = interval {
                    tokio::select! {
                        int = cinterval.tick() => {
                            log::info!("Interval tick at {int:?}");
                            timer_sender.send(TimerMsg::CaptureImage).unwrap();
                        }
                        Some(SetTimerMsg::Interval(d)) = set_timer_receiver.recv() => {
                            log::info!("new interval: {d:?}");
                            interval = d.map(|d| tokio::time::interval(d))
                        }
                    }
                } else {
                    match set_timer_receiver.recv().await {
                        Some(SetTimerMsg::Interval(d)) => {
                            log::info!("new interval: {d:?}");
                            interval = d.map(|d| tokio::time::interval(d));
                        }
                        None => (),
                    }
                }
            }
        })
    });

    //let timing_thread = std::thread::spawn(move || timer::run(timer_sender, set_timer_receiver));
    {
        let native_options = eframe::NativeOptions::default();
        eframe::run_native(
            "CamCap",
            native_options,
            Box::new(|cc| {
                let app = app::CamCapApp::new(cc, State::new(set_timer_sender, timer_receiver));
                Box::new(app)
            }),
        )
        .unwrap();
    }
}
