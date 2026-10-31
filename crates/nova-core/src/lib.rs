//! # NovaOS Core
//!
//! Ядро «своей операционной системы для телевизора» NovaOS. Здесь нет ни пикселей,
//! ни SDL — только чистая логика, которую можно тестировать на хосте и компилировать
//! под ARM soft-float (LG webOS):
//!
//! * [`ar`] — чтение/запись архивов `ar` (контейнер webOS `.ipk`);
//! * [`ipk`] — разбор и сборка пакетов webOS (IPK = Debian binary package 2.0);
//! * [`arsc`] — таблицы ресурсов Android (название приложения из `@string/...`);
//! * [`apk`] + [`axml`] — разбор Android-пакетов (APK) и бинарного `AndroidManifest.xml`;
//! * [`sniff`] — определение формата файла по магическим байтам;
//! * [`app`], [`catalog`] — модель приложения и каталог установленных приложений;
//! * [`luna`] — вызовы шины Luna (список приложений, запуск, установка, root-exec);
//! * [`repo`] — манифесты репозиториев в формате Homebrew Channel (`repo.json`);
//! * [`net`] — загрузка файлов с прогрессом и проверкой SHA-256;
//! * [`net`]/[`png`] — загрузка с SHA-256, кодировщик PNG и генератор иконок;
//! * [`install`] — оркестрация установки «своих приложений всех форматов»;
//! * [`webapp`] — упаковка HTML5/ZIP/M3U в полноценный webOS IPK;
//! * [`settings`], [`i18n`] — конфигурация и локализация (RU/EN);
//! * [`sysinfo`], [`home`] — сведения о ТВ и интеграция с кнопкой Home;
//! * [`qr`] — QR-коды (например, для установки приложения с телефона);
//! * [`upload`] — локальный HTTP-приёмник файлов (установка прямо с телефона);
//! * [`clock`] — местное время и дата без внешних зависимостей (разбор `TZif`).

pub mod apk;
pub mod app;
pub mod ar;
pub mod arsc;
pub mod axml;
pub mod catalog;
pub mod clock;
pub mod error;
pub mod home;
pub mod i18n;
pub mod install;
pub mod ipk;
pub mod luna;
pub mod net;
pub mod png;
pub mod qr;
pub mod repo;
pub mod settings;
pub mod sniff;
pub mod sysinfo;
pub mod upload;
pub mod webapp;

pub use error::{NovaError, NovaResult};

/// Версия ядра NovaOS (совпадает с версией пакета).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Идентификатор приложения лаунчера в системе webOS.
pub const APP_ID: &str = "com.novaos.launcher";

/// Название системы, отображаемое в интерфейсе.
pub const OS_NAME: &str = "NovaOS";
