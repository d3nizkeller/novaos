//! Утилита диагностики пакетов: `cargo run -p nova-core --example inspect -- <файл> [файл…]`
//!
//! Показывает, как NovaOS видит IPK/APK/ZIP/HTML/M3U — те же данные, которые
//! получает экран «Установка» на телевизоре.

use std::path::Path;

use nova_core::install::{InstallSource, Installer};
use nova_core::{apk, ipk, net, sniff};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Использование: inspect <пакет> [пакет…]");
        eprintln!("Поддерживаются: .ipk .apk .zip .html .m3u");
        std::process::exit(2);
    }
    for arg in &args {
        let path = Path::new(arg);
        println!("\n=== {} ===", path.display());
        match std::fs::read(path) {
            Ok(bytes) => inspect(path, &bytes),
            Err(error) => println!("не удалось прочитать: {error}"),
        }
    }
}

fn inspect(path: &Path, bytes: &[u8]) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let format = sniff::detect(bytes, Some(&name));
    println!("формат:      {}", format.label_ru());
    println!("действие:    {}", format.action_ru());
    println!("размер:      {}", net::format_size(bytes.len() as u64));
    println!("sha256:      {}", net::sha256_hex(bytes));

    match format {
        sniff::PackageFormat::Ipk | sniff::PackageFormat::LegacyIpk => match ipk::parse(bytes) {
            Ok(info) => {
                println!("приложение:  {} ({})", info.title(), info.app_id());
                println!("версия:      {}", info.version());
                println!(
                    "тип:         {} ({})",
                    info.app_type(),
                    if info.is_native() {
                        "нативное"
                    } else {
                        "веб"
                    }
                );
                println!("архитектура: {}", info.control.architecture);
                println!("файлов:      {}", info.file_count);
                println!("установка:   {}", net::format_size(info.installed_size));
                println!(
                    "сервис Luna: {}",
                    if info.has_service { "да" } else { "нет" }
                );
                if let Some(appinfo) = &info.appinfo {
                    println!("точка входа: {:?}", appinfo.main);
                    println!("категории:   {:?}", appinfo.app_category);
                    println!("ACG:         {:?}", appinfo.required_acg);
                }
                println!("--- первые 12 файлов ---");
                for entry in info.entries.iter().take(12) {
                    println!(
                        "  {}{:9}  {}",
                        if entry.is_dir { "d" } else { "-" },
                        entry.size,
                        entry.path
                    );
                }
                // Проверка, что пакет можно установить «вхолостую».
                let dir = std::env::temp_dir().join("nova-inspect");
                let installer = Installer::dry_run(dir);
                match installer.install(
                    &InstallSource::Memory {
                        name: name.clone(),
                        bytes: bytes.to_vec(),
                    },
                    None,
                    |_| {},
                ) {
                    Ok(report) => println!(
                        "готов к установке: {} {} [{}]",
                        report.title,
                        report.version,
                        report.strategy.label_ru()
                    ),
                    Err(error) => println!("ошибка подготовки: {error}"),
                }
            }
            Err(error) => println!("не удалось разобрать IPK: {error}"),
        },
        sniff::PackageFormat::Apk => match apk::parse(bytes) {
            Ok(info) => {
                println!("приложение:  {} ({})", info.display_title(), info.package);
                println!(
                    "версия:      {:?} (код {:?})",
                    info.version_name, info.version_code
                );
                println!("мин. SDK:    {:?}", info.min_sdk);
                println!("target SDK:  {:?}", info.target_sdk);
                println!("ABI:         {:?}", info.abis);
                println!(
                    "Android TV:  {}",
                    if info.is_tv_app() {
                        "да (leanback)"
                    } else {
                        "нет"
                    }
                );
                println!("телефон:     {}", info.phone_launcher);
                println!("разрешений:  {}", info.permissions.len());
                for permission in info.permissions.iter().take(8) {
                    println!("  - {permission}");
                }
                println!(
                    "элементов:   {} ({} распаковано)",
                    info.entry_count,
                    net::format_size(info.uncompressed_size)
                );
            }
            Err(error) => println!("не удалось разобрать APK: {error}"),
        },
        sniff::PackageFormat::Playlist => {
            let text = String::from_utf8_lossy(bytes);
            let channels = nova_core::webapp::parse_m3u(&text);
            println!("каналов:     {}", channels.len());
            for channel in channels.iter().take(5) {
                println!(
                    "  - {} [{}] {}",
                    channel.name,
                    channel.group.clone().unwrap_or_default(),
                    channel.url
                );
            }
        }
        sniff::PackageFormat::HtmlApp => {
            println!("байт HTML:   {}", bytes.len());
        }
        other => println!("(дополнительный разбор для {other:?} не требуется)"),
    }
}
