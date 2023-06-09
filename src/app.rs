use nokhwa::{
    native_api_backend,
    pixel_format::RgbFormat,
    query,
    utils::{CameraInfo, RequestedFormat, RequestedFormatType},
    Camera,
};
use std::{path::PathBuf, time::Duration};
use tokio::sync::mpsc::{error::TryRecvError, UnboundedReceiver, UnboundedSender};

use crate::timer::{SetTimerMsg, TimerMsg};

pub struct State {
    pub target_folder: PathBuf,
    pub cams: Vec<CameraInfo>,
    pub ccam: Option<Camera>,
    pub cframe_tex: Option<egui::TextureHandle>,
    pub timer_msg_recv: UnboundedReceiver<TimerMsg>,
    pub timer_set_msg_sender: UnboundedSender<SetTimerMsg>,
    pub timer_running: bool,
    pub timer_duration: Duration,
    pub image_nr: usize,
    pub timer_config: usize,
    pub preview_cam: bool,
}

impl State {
    pub fn new(
        set_timer_sender: UnboundedSender<SetTimerMsg>,
        msg_recv: UnboundedReceiver<TimerMsg>,
    ) -> Self {
        let cams = query(native_api_backend().expect("No native backend provided!"))
            .unwrap_or_else(|_| Vec::new());

        log::info!("cams:");
        for cam in &cams {
            log::info!("\tccam: {cam}");
        }
        let cam = cams.iter().next();
        let camera = cam.map(|cam| {
            let mut cam = Camera::new(
                cam.index().clone(),
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution),
            )
            .unwrap();
            cam.open_stream().ok();
            cam
        });

        Self {
            cams,
            ccam: camera,
            target_folder: Default::default(),
            cframe_tex: Default::default(),
            timer_msg_recv: msg_recv,
            timer_set_msg_sender: set_timer_sender,
            timer_running: false,
            timer_duration: Duration::from_secs(20),
            image_nr: 0,
            timer_config: 20,
            preview_cam: false,
        }
    }
}

pub struct CamCapApp {
    pub state: State,
}

fn calc_size_non_stretched(avail_size: [f32; 2], given_size: [f32; 2]) -> [f32; 2] {
    let [avail_w, avail_h] = avail_size;
    let [given_w, given_h] = given_size;
    let aspect_avail = avail_w / avail_h;
    let aspect_given = given_w / given_h;
    //log::info!("avail: [{aspect_avail}] given: [{aspect_given}]");

    // asp = w/h
    if aspect_avail > aspect_given {
        // viewport wider than image, align on height
        [avail_h * aspect_given, avail_h]
    } else {
        // viewport higher than image, align on width
        [avail_w, avail_w / aspect_given]
    }
}

impl CamCapApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, state: State) -> Self {
        Self { state }
    }

    fn capture_image(
        &mut self,
        ctx: &egui::Context,
        save_img_nr: Option<usize>,
    ) -> anyhow::Result<()> {
        let ccam = if let Some(ref mut ci) = self.state.ccam {
            ci
        } else {
            anyhow::bail!("No cam selected");
        };

        let ccam_res = ccam.resolution();

        let frame = ccam.frame()?.decode_image::<RgbFormat>()?;

        if let Some(nr) = save_img_nr {
            let mut target = self.state.target_folder.to_owned();
            target.push(format!("img_{nr:04}.png"));
            frame.save(target)?;
        }

        let img = egui::ColorImage::from_rgb(
            [ccam_res.width() as usize, ccam_res.height() as usize],
            &frame.into_raw(),
        );
        /*let img = match frame.decode_image::<RgbFormat>() {
        Ok(o) => o,
        Err(e) => {
        log::warn!("Error while decoding frame: {e}");
        return;
        }
        };*/

        self.state.cframe_tex = Some(ctx.load_texture("newest_shot", img, Default::default()));
        Ok(())
    }
}

impl eframe::App for CamCapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let cmsg = self.state.timer_msg_recv.try_recv();
        let image_changed = match (cmsg, self.state.preview_cam) {
            (Ok(TimerMsg::CaptureImage), _) | (_, true) => {
                if self.state.preview_cam && !matches!(cmsg, Ok(TimerMsg::CaptureImage)) {
                    self.capture_image(ctx, None).ok();
                    true
                } else {
                    match self.capture_image(ctx, Some(self.state.image_nr)) {
                        Ok(()) => {
                            self.state.image_nr += 1;
                            true
                        }
                        Err(e) => {
                            log::warn!("Error while capturing image: {e}");
                            false
                        }
                    }
                }
            }
            (Err(TryRecvError::Empty), _) => false,
            (Err(TryRecvError::Disconnected), _) => panic!("{}", TryRecvError::Disconnected),
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                if ui.button("Select folder").clicked() {
                    if let Some(new_path) = rfd::FileDialog::new().set_directory(&self.state.target_folder).pick_folder() {
                        self.state.target_folder = new_path;
                    }
                }

                ui.menu_button("Camera", |ui| {
                    if ui.button("refresh cameras").clicked() {
                        self.state.cams =
                            query(native_api_backend().expect("No native backend provided!"))
                                .unwrap_or_else(|_| Vec::new());
                    }
                    ui.separator();
                    for cam in &self.state.cams {
                        if ui.button(format!("{}", cam.human_name())).clicked() {
                            let mut cam = Camera::new(
                                cam.index().clone(),
                                RequestedFormat::new::<RgbFormat>(
                                    RequestedFormatType::AbsoluteHighestResolution,
                                ),
                            )
                            .unwrap();
                            cam.open_stream().ok();
                            self.state.ccam = Some(cam);
                        }
                    }
                });
                if self.state.timer_running {
                    if ui.button("stop capturing").clicked() {
                        log::info!("sending stop capturing");
                        self.state.timer_running = false;
                        self.state
                            .timer_set_msg_sender
                            .send(SetTimerMsg::Interval(None))
                            .ok();
                    }
                } else {
                    if ui.button("start capturing").clicked() {
                        log::info!(
                            "sending start capturing with duration: {:?}",
                            self.state.timer_duration
                        );
                        self.state.timer_running = true;
                        self.state
                            .timer_set_msg_sender
                            .send(SetTimerMsg::Interval(Some(self.state.timer_duration)))
                            .ok();
                    }
                }

                if self.state.preview_cam {
                    if ui.button("turn off preview").clicked() {
                        self.state.preview_cam = false;
                    }
                } else {
                    if ui.button("turn on preview").clicked() {
                        self.state.preview_cam = true;
                    }
                }

                ui.add(egui::DragValue::new(&mut self.state.image_nr));
                ui.separator();

                ui.add(
                    egui::Slider::new(&mut self.state.timer_config, 1..=(60 * 60 * 24))
                        .step_by(1.0)
                        .logarithmic(true)
                        .show_value(true)
                        .custom_formatter(|n, _| {
                            let n = n as i32;
                            let hours = n / (60 * 60);
                            let mins = (n / 60) % 60;
                            let secs = n % 60;
                            format!("{hours:02}:{mins:02}:{secs:02}")
                        })
                        .custom_parser(|s| {
                            let parts: Vec<&str> = s.split(':').collect();
                            if parts.len() == 3 {
                                parts[0]
                                    .parse::<i32>()
                                    .and_then(|h| {
                                        parts[1].parse::<i32>().and_then(|m| {
                                            parts[2]
                                                .parse::<i32>()
                                                .map(|s| ((h * 60 * 60) + (m * 60) + s) as f64)
                                        })
                                    })
                                    .ok()
                            } else {
                                None
                            }
                        }),
                );
                self.state.timer_duration = Duration::from_secs(self.state.timer_config as u64);
            });

            //if self.cframe_tex.read().unwrap().is_none() {
            //}

            if let Some(ref cframe_tex) = self.state.cframe_tex {
                let avail_size = ui.available_size_before_wrap();
                let [new_size_w, new_size_h] = calc_size_non_stretched(
                    [avail_size.x, avail_size.y],
                    [cframe_tex.size()[0] as f32, cframe_tex.size()[1] as f32],
                );

                ui.allocate_ui(egui::Vec2::new(new_size_w, new_size_h), |ui| {
                    egui::Image::new(cframe_tex, egui::Vec2::new(new_size_w, new_size_h))
                        .paint_at(ui, ui.available_rect_before_wrap());
                });
            }
        });

        if image_changed {
            ctx.request_repaint();
        }
    }
}
