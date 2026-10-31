//! Бэкенд SDL2: окно на desktop и полноэкранный вывод на телевизоре.
//!
//! На LG webOS собирается та же ветка, но с SDL-webOS (Wayland/EGL): пакет
//! `sdl2` указывает на sysroot тулчейна, а буфер кадра уходит в потоковую
//! текстуру без промежуточных копий в память GPU.

use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;

use super::{Loop, Options};
use crate::screens::Key;
use crate::state::{AppState, Modal};

/// Запускает оконный цикл NovaOS до выхода из приложения.
pub fn run(state: &mut AppState, options: Options) -> Result<u64, String> {
    let sdl = sdl2::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;

    let window = video
        .window("NovaOS", options.width, options.height)
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_streaming(PixelFormatEnum::ABGR8888, options.width, options.height)
        .map_err(|e| e.to_string())?;

    let mut pump = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut loop_ = Loop::new(options);
    let frame_time = loop_.options.frame_duration();
    let mut last = Instant::now();

    while !loop_.done(state) {
        let started = Instant::now();
        for event in pump.poll_iter() {
            match event {
                Event::Quit { .. } => state.quit = true,
                Event::KeyDown {
                    keycode: Some(code),
                    repeat: false,
                    ..
                } => {
                    if let Some(key) = map_key(code) {
                        crate::screens::handle_key(state, key);
                    }
                }
                Event::TextInput { text, .. } => {
                    if matches!(state.modal, Modal::Keyboard(_)) {
                        for ch in text.chars() {
                            state.modal_push_char(ch);
                        }
                    }
                }
                Event::Window { .. } | Event::MouseMotion { .. } | Event::MouseWheel { .. } => {}
                _ => {}
            }
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.25);
        last = now;

        loop_.step(state, dt);

        let stride = loop_.options.width as usize * 4;
        texture
            .update(None, &loop_.renderer.canvas.pixels, stride)
            .map_err(|e| e.to_string())?;
        canvas
            .copy(&texture, None, None)
            .map_err(|e| e.to_string())?;
        canvas.present();

        if loop_.options.realtime {
            if let Some(left) = frame_time.checked_sub(started.elapsed()) {
                std::thread::sleep(left);
            }
        }
    }
    Ok(loop_.frame)
}

/// Клавиша SDL → клавиша пульта NovaOS.
fn map_key(code: Keycode) -> Option<Key> {
    let mapped = match code {
        Keycode::Left | Keycode::A => Key::Left,
        Keycode::Right | Keycode::D => Key::Right,
        Keycode::Up | Keycode::W => Key::Up,
        Keycode::Down | Keycode::S => Key::Down,
        Keycode::Return | Keycode::KpEnter | Keycode::Space => Key::Ok,
        Keycode::Escape | Keycode::Backspace => Key::Back,
        Keycode::Tab => Key::Menu,
        Keycode::Q => Key::Exit,
        Keycode::P => Key::Play,
        Keycode::R | Keycode::F1 => Key::Red,
        Keycode::G | Keycode::F2 => Key::Green,
        Keycode::Y | Keycode::F3 => Key::Yellow,
        Keycode::B | Keycode::F4 => Key::Blue,
        Keycode::I | Keycode::F6 => Key::Info,
        Keycode::F5 => Key::Refresh,
        Keycode::Num0 | Keycode::Kp0 => Key::Digit(0),
        Keycode::Num1 | Keycode::Kp1 => Key::Digit(1),
        Keycode::Num2 | Keycode::Kp2 => Key::Digit(2),
        Keycode::Num3 | Keycode::Kp3 => Key::Digit(3),
        Keycode::Num4 | Keycode::Kp4 => Key::Digit(4),
        Keycode::Num5 | Keycode::Kp5 => Key::Digit(5),
        Keycode::Num6 | Keycode::Kp6 => Key::Digit(6),
        Keycode::Num7 | Keycode::Kp7 => Key::Digit(7),
        Keycode::Num8 | Keycode::Kp8 => Key::Digit(8),
        Keycode::Num9 | Keycode::Kp9 => Key::Digit(9),
        other => Key::from_name(&other.name())?,
    };
    Some(mapped)
}

/// Пауза до конца кадра (используется тестами окна).
pub fn frame_pause(target: Duration, elapsed: Duration) -> Option<Duration> {
    target.checked_sub(elapsed)
}
