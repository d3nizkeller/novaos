//! Бэкенды вывода кадров NovaOS.
//!
//! Оба бэкенда крутят один и тот же цикл: нажатия пульта → [`handle_key`],
//! [`AppState::tick`] для часов и анимаций, [`Renderer::frame`] для кадра.
//! Разница только в том, куда уходит готовый RGBA-буфер: в PNG-файлы
//! (`offscreen`, без окна) или в текстуру SDL2 (`sdl`, окно на desktop и на
//! телевизоре через SDL-webOS).

pub mod offscreen;
#[cfg(feature = "sdl")]
pub mod sdl;

use std::path::PathBuf;
use std::time::Duration;

use crate::screens::{handle_key, Key, Renderer};
use crate::state::AppState;

/// Параметры запуска цикла отрисовки.
#[derive(Debug, Clone)]
pub struct Options {
    /// Ширина кадра в пикселях.
    pub width: u32,
    /// Высота кадра в пикселях.
    pub height: u32,
    /// Целевая частота кадров.
    pub fps: u32,
    /// Ограничение числа кадров (`None` — работать до выхода).
    pub frames: Option<u64>,
    /// Каталог для PNG-снимков (`None` — не сохранять).
    pub out_dir: Option<PathBuf>,
    /// Сохранять каждый N-й кадр.
    pub every: u64,
    /// Сценарий нажатий: имена клавиш через запятую.
    pub keys: Vec<String>,
    /// Кадров между нажатиями сценария.
    pub key_period: u64,
    /// Держать реальное время кадра (пауза до конца кадра).
    pub realtime: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60,
            frames: None,
            out_dir: None,
            every: 30,
            keys: Vec::new(),
            key_period: 20,
            realtime: true,
        }
    }
}

impl Options {
    /// Длительность одного кадра.
    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.fps.max(1)))
    }
}

/// Общий цикл: состояние, рендерер, счётчик кадров и снимки.
pub struct Loop {
    /// Рендерер кадра.
    pub renderer: Renderer,
    /// Параметры запуска.
    pub options: Options,
    /// Номер текущего кадра.
    pub frame: u64,
    /// Индекс следующего нажатия сценария.
    script: usize,
}

impl Loop {
    /// Создаёт цикл и рендерер под размеры из параметров.
    pub fn new(options: Options) -> Self {
        let renderer = Renderer::new(options.width, options.height);
        Self {
            renderer,
            options,
            frame: 0,
            script: 0,
        }
    }

    /// Клавиша сценария для текущего кадра (если пора).
    pub fn script_key(&mut self) -> Option<Key> {
        let period = self.options.key_period.max(1);
        while self.script < self.options.keys.len() && self.frame >= self.script as u64 * period {
            let name = self.options.keys[self.script].clone();
            self.script += 1;
            if let Some(key) = Key::from_name(&name) {
                return Some(key);
            }
        }
        None
    }

    /// Один шаг цикла: тик состояния и новый кадр.
    pub fn step(&mut self, state: &mut AppState, dt: f32) {
        state.tick(dt);
        self.renderer.frame(state);
        self.save(state);
        self.frame += 1;
    }

    fn save(&self, state: &AppState) {
        let Some(dir) = &self.options.out_dir else {
            return;
        };
        let every = self.options.every.max(1);
        if self.frame % every != 0 {
            return;
        }
        let _ = state;
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let path = dir.join(format!("frame-{:05}.png", self.frame));
        if let Err(error) = self.renderer.save_png(&path) {
            eprintln!(
                "nova: не удалось сохранить кадр {}: {error}",
                path.display()
            );
        }
    }

    /// Завершать ли цикл.
    pub fn done(&self, state: &AppState) -> bool {
        state.quit || self.options.frames.is_some_and(|limit| self.frame >= limit)
    }
}

/// Применяет клавишу сценария, если она есть на этом кадре.
pub fn apply_script(loop_: &mut Loop, state: &mut AppState) {
    if let Some(key) = loop_.script_key() {
        handle_key(state, key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, WorkerEnv};
    use nova_core::settings::Settings;
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    fn state() -> (AppState, MutexGuard<'static, ()>) {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("nova-backend-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        std::env::set_var("NOVA_HOME", &dir);
        let mut settings = Settings::default();
        settings.work_dir = Some(dir.join("work").to_string_lossy().into_owned());
        settings.screensaver_minutes = 0;
        (AppState::inline(settings, WorkerEnv::sandbox(&dir)), guard)
    }

    #[test]
    fn offscreen_loop_runs_script_and_stops_at_limit() {
        let (mut state, _guard) = state();
        let dir = std::env::temp_dir().join(format!("nova-backend-out-{}", std::process::id()));
        let options = Options {
            width: 320,
            height: 180,
            fps: 60,
            frames: Some(45),
            out_dir: Some(dir.clone()),
            every: 15,
            keys: vec!["Right".into(), "Ok".into()],
            key_period: 10,
            realtime: false,
        };
        let frames = offscreen::run(&mut state, options);
        assert_eq!(frames, 45);
        assert_eq!(
            state.screen,
            crate::state::Screen::Apps,
            "сценарий дошёл до «Приложений»"
        );
        let saved: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .collect();
        assert_eq!(saved.len(), 3, "кадры 0, 15, 30");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loop_reports_script_keys_on_schedule() {
        let (mut state, _guard) = state();
        let mut loop_ = Loop::new(Options {
            width: 160,
            height: 90,
            frames: Some(4),
            keys: vec!["Down".into(), "nope".into(), "Back".into()],
            key_period: 1,
            realtime: false,
            ..Options::default()
        });
        assert_eq!(loop_.script_key(), Some(Key::Down));
        loop_.step(&mut state, 0.016);
        // Кадр 1: имя «nope» пропускается, но своё время уже заняло.
        assert_eq!(loop_.script_key(), None);
        loop_.step(&mut state, 0.016);
        assert_eq!(loop_.script_key(), Some(Key::Back));
        assert!(!loop_.done(&state));
        for _ in 0..4 {
            loop_.step(&mut state, 0.016);
        }
        assert!(loop_.done(&state), "лимит кадров");
    }
}
