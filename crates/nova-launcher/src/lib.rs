//! NovaOS — нативный лаунчер для LG webOS.
//!
//! Библиотечная часть содержит всё, что можно протестировать без окна:
//! программный рендерер ([`gfx`]), дизайн-систему ([`theme`]), виджеты
//! ([`widgets`]), состояние приложения ([`state`]) и экраны ([`screens`]).
//! Бэкенды вывода ([`backend`]) — заглушка в PNG (`offscreen`) и окно SDL2
//! (`sdl`, на телевизоре — SDL-webOS с Wayland).

pub mod backend;
#[cfg(all(target_os = "linux", target_arch = "arm"))]
pub mod compat;
pub mod gfx;
pub mod screens;
pub mod state;
pub mod theme;
pub mod widgets;
