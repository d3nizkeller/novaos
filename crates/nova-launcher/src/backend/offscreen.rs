//! Головныйless-бэкенд: кадры уходят в PNG, окно не создаётся.
//!
//! Используется в CI (проверка, что все экраны рисуются без паники), для
//! генерации скриншотов документации и для desktop-превью на машине без
//! графической оболочки.

use std::time::Instant;

use super::{apply_script, Loop, Options};
use crate::state::AppState;

/// Крутит цикл без окна и возвращает число отрисованных кадров.
pub fn run(state: &mut AppState, options: Options) -> u64 {
    let mut loop_ = Loop::new(options);
    let dt = 1.0 / f64::from(loop_.options.fps.max(1)) as f32;
    let frame_time = loop_.options.frame_duration();

    while !loop_.done(state) {
        let started = Instant::now();
        apply_script(&mut loop_, state);
        loop_.step(state, dt);
        if loop_.options.realtime {
            if let Some(left) = frame_time.checked_sub(started.elapsed()) {
                std::thread::sleep(left);
            }
        }
    }
    loop_.frame
}
