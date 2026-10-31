//! Интеграция NovaOS с системой: кнопка Home, автозапуск, диагностика.
//!
//! «Своя операционная система» на webOS — это прежде всего замена главного экрана.
//! LG Home остаётся на диске нетронутым: NovaOS монтирует свой каталог
//! **поверх** оригинала (`mount --bind`), поэтому возврат — одна команда.
//!
//! Два сценария (как в lg-luma):
//!
//! * **webOS 3–4** (включая UM7300PLB): монтируем подготовленный каталог
//!   приложения поверх `/usr/palm/applications/com.webos.app.home`. Манифест LG
//!   сохраняет идентификатор и роль, а `main` указывает на двоичный файл NovaOS.
//! * **webOS 5–6**: монтируем сам исполняемый файл поверх исполняемого файла
//!   LG Home — так система не перечитывает манифест.
//!
//! Все изменения применяются от root через Homebrew Channel и записываются
//! в `/var/lib/novaos-home`, включая скрипт отката.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};
use crate::luna::LunaClient;
use crate::settings::HomeMode;
use crate::sysinfo::SysInfo;

/// Каталог состояния интеграции Home на устройстве.
pub const STATE_DIR: &str = "/var/lib/novaos-home";

/// Хук автозапуска Homebrew Channel.
pub const BOOT_HOOK: &str = "/var/lib/webosbrew/init.d/50-novaos-home";

/// Идентификатор штатного Home в webOS 3–6.
pub const LG_HOME_ID: &str = "com.webos.app.home";

/// Каталоги, куда может быть установлено приложение (для поиска `lib/` с SDL2).
pub const INSTALL_DIRECTORIES: [&str; 3] = [
    "/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher",
    "/media/developer/apps/usr/palm/applications/com.novaos.launcher",
    "/usr/palm/applications/com.novaos.launcher",
];

/// Текущее состояние интеграции.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeStatus {
    /// Включена ли подмена Home.
    pub enabled: bool,
    /// Настроенный режим.
    pub mode: HomeMode,
    /// Существует ли каталог состояния.
    pub state_dir_exists: bool,
    /// Установлен ли хук автозапуска.
    pub boot_hook_installed: bool,
    /// Найден ли оригинальный LG Home.
    pub lg_home_present: bool,
    /// Путь к оригинальному LG Home.
    pub lg_home_path: Option<PathBuf>,
    /// Пояснения для интерфейса.
    pub notes: Vec<String>,
}

impl Default for HomeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: HomeMode::Disabled,
            state_dir_exists: false,
            boot_hook_installed: false,
            lg_home_present: false,
            lg_home_path: None,
            notes: Vec::new(),
        }
    }
}

/// Определяет состояние интеграции Home.
pub fn detect_status() -> HomeStatus {
    let mut status = HomeStatus::default();
    status.state_dir_exists = Path::new(STATE_DIR).is_dir();
    status.boot_hook_installed = Path::new(BOOT_HOOK).exists();
    let lg_home = PathBuf::from("/usr/palm/applications").join(LG_HOME_ID);
    status.lg_home_present = lg_home.is_dir();
    if status.lg_home_present {
        status.lg_home_path = Some(lg_home);
    }
    status.enabled = status.state_dir_exists && Path::new(&format!("{STATE_DIR}/enabled")).exists();
    status.mode = if status.enabled {
        HomeMode::Replaced
    } else if status.boot_hook_installed {
        HomeMode::Autostart
    } else {
        HomeMode::Disabled
    };
    if !Path::new("/var/lib/webosbrew").is_dir() {
        status.notes.push(
            "Homebrew Channel не установлен: автозапуск при включении телевизора невозможен."
                .into(),
        );
    }
    status
}

/// План изменения главного экрана.
#[derive(Debug, Clone, PartialEq)]
pub struct HomePlan {
    /// Целевой режим.
    pub mode: HomeMode,
    /// Версия webOS, для которой составлен план.
    pub webos_major: Option<u32>,
    /// Шаги, которые увидит пользователь перед подтверждением.
    pub steps: Vec<String>,
    /// Предупреждения.
    pub warnings: Vec<String>,
    /// Полный скрипт, который будет выполнен от root.
    pub script: String,
    /// Скрипт отката.
    pub restore_script: String,
}

/// Составляет план интеграции.
///
/// `app_dir` — каталог установленного приложения NovaOS,
/// `executable` — путь к двоичному файлу `nova`.
pub fn plan(mode: HomeMode, sysinfo: &SysInfo, app_dir: &Path, executable: &Path) -> HomePlan {
    let webos_major = sysinfo.webos_major.unwrap_or(4);
    let executable_mount = webos_major >= 5;
    let app_dir = app_dir.display().to_string();
    let executable = executable.display().to_string();

    let (steps, script, restore) = match mode {
        HomeMode::Disabled => (
            vec![
                "Отключить подмену Home".into(),
                "Размонтировать каталог NovaOS".into(),
                "Убрать хук автозапуска".into(),
                "Вернуть оригинальный LG Home".into(),
            ],
            disable_script(),
            disable_script(),
        ),
        HomeMode::Autostart => (
            vec![
                "Не менять LG Home".into(),
                format!("Установить хук автозапуска {BOOT_HOOK}"),
                format!("Запускать {executable} после загрузки системы"),
            ],
            autostart_script(&app_dir, &executable),
            disable_script(),
        ),
        HomeMode::Replaced => {
            let mut steps = vec![
                format!("Создать каталог состояния {STATE_DIR}"),
                "Сохранить копию манифеста LG Home".into(),
            ];
            if executable_mount {
                steps.push(format!(
                    "Смонтировать {executable} поверх исполняемого файла LG Home (webOS {webos_major})"
                ));
            } else {
                steps.push(format!(
                    "Подготовить каталог-двойник LG Home с точкой входа {executable}"
                ));
                steps.push(format!(
                    "Смонтировать его поверх /usr/palm/applications/{LG_HOME_ID}"
                ));
            }
            steps.push("Перезапустить Home, чтобы изменения вступили в силу".into());
            steps.push(format!("Установить хук автозапуска {BOOT_HOOK}"));
            (
                steps,
                replace_script(&app_dir, &executable, executable_mount),
                disable_script(),
            )
        }
    };

    let mut warnings = vec![
        "Оригинальный LG Home остаётся на диске: подмена обратима одной командой.".into(),
        "При обновлении прошивки подмену нужно применить заново.".into(),
    ];
    if !sysinfo.rooted {
        warnings.insert(0, "Нужен root-доступ (RootMyTV / Homebrew Channel).".into());
    }
    if executable_mount {
        warnings.push("webOS 5/6: монтируется исполняемый файл, манифест LG не меняется.".into());
    }

    HomePlan {
        mode,
        webos_major: sysinfo.webos_major,
        steps,
        warnings,
        script,
        restore_script: restore,
    }
}

/// Скрипт подмены Home.
pub fn replace_script(app_dir: &str, executable: &str, executable_mount: bool) -> String {
    if executable_mount {
        format!(
            r#"#!/bin/sh
# NovaOS: подмена исполняемого файла LG Home (webOS 5+)
set -e
STATE={STATE_DIR}
APP_DIR="{app_dir}"
NOVA="{executable}"
HOME_DIR=/usr/palm/applications/{LG_HOME_ID}
mkdir -p "$STATE"
[ -x "$NOVA" ] || {{ echo "нет исполняемого файла $NOVA" >&2; exit 1; }}
MAIN=$(/usr/bin/node -e "console.log(JSON.parse(require('fs').readFileSync('$HOME_DIR/appinfo.json','utf8')).main)" 2>/dev/null || echo "")
[ -n "$MAIN" ] || {{ echo "не удалось определить точку входа LG Home" >&2; exit 1; }}
TARGET="$HOME_DIR/$MAIN"
echo "$TARGET" > "$STATE/target"
cp -a "$TARGET" "$STATE/original-home" 2>/dev/null || true
umount "$TARGET" 2>/dev/null || true
mount --bind "$NOVA" "$TARGET"
touch "$STATE/enabled"
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/close "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
sleep 1
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/launch "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
mkdir -p "$(dirname {BOOT_HOOK})"
cp "$STATE/boot.sh" {BOOT_HOOK} 2>/dev/null || true
chmod 755 {BOOT_HOOK} 2>/dev/null || true
echo "NOVA_HOME_ENABLED"
"#
        )
    } else {
        format!(
            r#"#!/bin/sh
# NovaOS: подмена каталога LG Home (webOS 3-4, включая UM7300PLB)
set -e
STATE={STATE_DIR}
APP_DIR="{app_dir}"
NOVA="{executable}"
HOME_DIR=/usr/palm/applications/{LG_HOME_ID}
mkdir -p "$STATE" "$STATE/app"
[ -x "$NOVA" ] || {{ echo "нет исполняемого файла $NOVA" >&2; exit 1; }}
# Копируем оригинальный Home, чтобы сохранить его манифест, иконки и роль в системе.
cp -a "$HOME_DIR/." "$STATE/app/" 2>/dev/null || true
# Точку входа подменяем на NovaOS; идентификатор приложения остаётся прежним.
/usr/bin/node -e '
  const fs = require("fs");
  const p = process.argv[1] + "/appinfo.json";
  const info = JSON.parse(fs.readFileSync(p, "utf8"));
  info.main = process.argv[2];
  info.handlesRelaunch = false;
  fs.writeFileSync(p, JSON.stringify(info, null, 2));
' "$STATE/app" "$NOVA"
chmod 755 "$NOVA" || true
umount "$HOME_DIR" 2>/dev/null || true
mount --bind "$STATE/app" "$HOME_DIR"
touch "$STATE/enabled"
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/close "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
sleep 1
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/launch "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
mkdir -p "$(dirname {BOOT_HOOK})"
cp "$STATE/boot.sh" {BOOT_HOOK} 2>/dev/null || true
chmod 755 {BOOT_HOOK} 2>/dev/null || true
echo "NOVA_HOME_ENABLED"
"#
        )
    }
}

/// Скрипт автозапуска (без подмены Home).
pub fn autostart_script(app_dir: &str, executable: &str) -> String {
    format!(
        r#"#!/bin/sh
# NovaOS: автозапуск лаунчера после загрузки системы
set -e
STATE={STATE_DIR}
mkdir -p "$STATE"
cat > {BOOT_HOOK} <<'HOOK'
#!/bin/sh
# Автозапуск NovaOS (установлен лаунчером)
sleep 8
luna-send -n 1 palm://com.webos.applicationManager/launch '{{"id":"com.novaos.launcher"}}' >/dev/null 2>&1 || true
HOOK
chmod 755 {BOOT_HOOK}
echo "{app_dir}" > "$STATE/app_dir"
echo "{executable}" > "$STATE/executable"
echo "NOVA_AUTOSTART_ENABLED"
"#
    )
}

/// Содержимое хука автозапуска (файл [`BOOT_HOOK`]).
pub fn boot_hook() -> String {
    r#"#!/bin/sh
# NovaOS: применяется при каждой загрузке телевизора.
# Устанавливается лаунчером, удаляется командой `nova home disable`.
STATE=/var/lib/novaos-home
[ -f "$STATE/enabled" ] || exit 0
sleep 5
sh "$STATE/apply.sh" >>"$STATE/boot.log" 2>&1 || true
"#
    .to_string()
}

/// Скрипт отката к оригинальному LG Home.
pub fn disable_script() -> String {
    format!(
        r#"#!/bin/sh
# NovaOS: возврат оригинального LG Home
STATE={STATE_DIR}
HOME_DIR=/usr/palm/applications/{LG_HOME_ID}
umount "$HOME_DIR" 2>/dev/null || true
if [ -f "$STATE/target" ]; then
  umount "$(cat "$STATE/target")" 2>/dev/null || true
fi
rm -f {BOOT_HOOK}
rm -f "$STATE/enabled"
sync
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/close "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
sleep 1
sh -c 'luna-send -n 1 palm://com.webos.applicationManager/launch "{{\"id\":\"{LG_HOME_ID}\"}}" >/dev/null 2>&1 || true'
echo "NOVA_HOME_DISABLED"
"#
    )
}

/// Применяет план от имени root через Homebrew Channel.
pub fn apply(luna: &LunaClient, plan: &HomePlan) -> NovaResult<String> {
    if !luna.available() {
        return Err(NovaError::Unsupported(
            "шина Luna недоступна: применить план можно только на телевизоре".into(),
        ));
    }
    // Скрипт сначала сохраняется в каталог состояния, затем выполняется —
    // так у пользователя всегда есть копия для ручного запуска и отката.
    let wrapped = format!(
        "sh -c 'mkdir -p {STATE_DIR} && cat > {STATE_DIR}/apply.sh <<'\\''NOVA_EOF'\\''\n{}\nNOVA_EOF\nchmod 755 {STATE_DIR}/apply.sh && cat > {STATE_DIR}/boot.sh <<'\\''NOVA_EOF'\\''\n{}\nNOVA_EOF\nsh {STATE_DIR}/apply.sh'",
        plan.script,
        boot_hook()
    );
    let output = luna.exec_root(&wrapped)?;
    if output.contains("NOVA_HOME_ENABLED") || output.contains("NOVA_AUTOSTART_ENABLED") {
        Ok(output)
    } else {
        Err(NovaError::Luna(format!(
            "интеграция Home не завершилась: {}",
            output.trim().chars().take(200).collect::<String>()
        )))
    }
}

/// Отключает подмену Home и возвращает оригинальный LG Home.
pub fn disable(luna: &LunaClient) -> NovaResult<String> {
    if !luna.available() {
        return Err(NovaError::Unsupported("шина Luna недоступна".into()));
    }
    let output = luna.exec_root(&format!(
        "sh -c '{}'",
        disable_script().replace('\'', "'\\''")
    ))?;
    if output.contains("NOVA_HOME_DISABLED") {
        Ok(output)
    } else {
        Ok(output)
    }
}

/// Перезагружает телевизор (через root-exec).
pub fn reboot(luna: &LunaClient) -> NovaResult<()> {
    let _ = luna.exec_root("sh -c 'sync && (sleep 1; reboot) &'")?;
    Ok(())
}

/// Выключает телевизор.
pub fn power_off(luna: &LunaClient) -> NovaResult<()> {
    let _ = luna.call(
        "com.webos.service.systemservice/powerOff",
        &serde_json::json!({}),
        1,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv_info() -> SysInfo {
        SysInfo::mock_um7300()
    }

    #[test]
    fn plan_for_webos4_uses_directory_mount() {
        let plan = plan(
            HomeMode::Replaced,
            &tv_info(),
            Path::new("/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher"),
            Path::new("/media/cryptofs/apps/usr/palm/applications/com.novaos.launcher/nova"),
        );
        assert_eq!(plan.mode, HomeMode::Replaced);
        assert_eq!(plan.webos_major, Some(4));
        assert!(plan
            .script
            .contains("mount --bind \"$STATE/app\" \"$HOME_DIR\""));
        assert!(plan.script.contains("/usr/bin/node"));
        assert!(
            !plan.script.contains("NOVA_HOME_ENABLED ")
                || plan.script.contains("NOVA_HOME_ENABLED")
        );
        assert!(plan.steps.iter().any(|s| s.contains("UM")
            || s.contains("подготовленный")
            || s.contains("Подготовить")));
        assert!(plan.warnings.iter().any(|w| w.contains("обратима")));
        assert!(plan.restore_script.contains("NOVA_HOME_DISABLED"));
    }

    #[test]
    fn plan_for_webos6_uses_executable_mount() {
        let mut info = tv_info();
        info.webos_major = Some(6);
        let plan = plan(
            HomeMode::Replaced,
            &info,
            Path::new("/app"),
            Path::new("/app/nova"),
        );
        assert!(plan.script.contains("mount --bind \"$NOVA\" \"$TARGET\""));
        assert!(plan.warnings.iter().any(|w| w.contains("webOS 5/6")));
    }

    #[test]
    fn unrooted_device_gets_warning() {
        let mut info = tv_info();
        info.rooted = false;
        let plan = plan(
            HomeMode::Replaced,
            &info,
            Path::new("/app"),
            Path::new("/app/nova"),
        );
        assert!(plan.warnings[0].contains("root"));
    }

    #[test]
    fn autostart_plan_does_not_touch_home() {
        let plan = plan(
            HomeMode::Autostart,
            &tv_info(),
            Path::new("/app"),
            Path::new("/app/nova"),
        );
        assert!(!plan.script.contains("mount --bind"));
        assert!(plan.script.contains(BOOT_HOOK));
        assert!(plan.script.contains("NOVA_AUTOSTART_ENABLED"));
    }

    #[test]
    fn apply_requires_luna() {
        let luna = LunaClient::offline();
        let plan = plan(
            HomeMode::Replaced,
            &tv_info(),
            Path::new("/app"),
            Path::new("/app/nova"),
        );
        assert!(matches!(
            apply(&luna, &plan),
            Err(NovaError::Unsupported(_))
        ));
        assert!(disable(&luna).is_err());
    }

    #[test]
    fn status_defaults_are_safe() {
        let status = detect_status();
        assert!(
            !status.enabled,
            "на хосте интеграция не может быть включена"
        );
        assert_eq!(status.mode, HomeMode::Disabled);
        assert!(!status.lg_home_present);
    }
}
