//! Превращение «своих» приложений в пакеты webOS.
//!
//! webOS понимает только IPK с `appinfo.json`. Поэтому, когда пользователь
//! приносит HTML-файл, ZIP-архив или плейлист M3U, NovaOS упаковывает их
//! в полноценное приложение:
//!
//! | Источник | Что получается |
//! |---|---|
//! | `index.html` / одиночный HTML | веб-приложение webOS |
//! | ZIP с веб-приложением | веб-приложение webOS (архив распаковывается) |
//! | каталог с `appinfo.json` | нативное/веб-приложение как есть |
//! | `playlist.m3u` | встроенный IPTV-плеер NovaOS с этим плейлистом |
//!
//! Иконки (`icon.png` 80×80 и `app-icon-large.png` 130×130) генерируются
//! детерминированно из идентификатора, поэтому у приложения всегда есть лицо.

use std::io::Read;
use std::path::Path;

use serde_json::json;

use crate::error::{NovaError, NovaResult};
use crate::ipk::{self, ControlFile, PackageFile};
use crate::png;

/// Описание будущего приложения.
#[derive(Debug, Clone, PartialEq)]
pub struct WebAppSpec {
    /// Идентификатор приложения webOS.
    pub id: String,
    /// Название в интерфейсе.
    pub title: String,
    /// Версия.
    pub version: String,
    /// Издатель.
    pub vendor: String,
    /// Описание.
    pub description: String,
    /// Цвет иконки (`#rrggbb`).
    pub icon_color: String,
}

impl WebAppSpec {
    /// Создаёт спецификацию из имени файла/приложения.
    ///
    /// Расширение отбрасывается: `Super Player.zip` → `com.novaos.side.super-player`.
    pub fn from_name(name: &str) -> Self {
        let stem = Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string());
        let mut slug = slugify(&stem);
        if slug == "app" && stem != name {
            // Имя без расширения не дало ASCII-символов (например, кириллица) —
            // пробуем полное имя, чтобы сохранить хотя бы расширение.
            slug = slugify(name);
        }
        Self {
            id: format!("com.novaos.side.{slug}"),
            title: cleanup_title(name),
            version: "1.0.0".into(),
            vendor: "NovaOS Sideload".into(),
            description: format!("Установлено через NovaOS из «{}»", name),
            icon_color: "#1b6ef3".into(),
        }
    }

    /// Задаёт название.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Задаёт версию.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Задаёт идентификатор (проверяется на корректность).
    pub fn with_id(mut self, id: impl Into<String>) -> NovaResult<Self> {
        let id = id.into();
        if !valid_app_id(&id) {
            return Err(NovaError::Invalid(format!(
                "некорректный идентификатор приложения: {id} (ожидается формат com.example.app)"
            )));
        }
        self.id = id;
        Ok(self)
    }
}

/// Приводит произвольное имя к slug для идентификатора.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            slug.push('-');
        } else if ch.is_whitespace() || ch == '.' {
            slug.push('-');
        }
        // Кириллица и прочие символы отбрасываются: идентификатор webOS должен быть ASCII.
    }
    let slug = slug.trim_matches('-').to_string();
    let slug = slug.replace("--", "-");
    if slug.is_empty() {
        "app".to_string()
    } else if slug.chars().next().map(|c| c.is_ascii_digit()) == Some(true) {
        format!("a-{slug}")
    } else {
        slug.chars().take(40).collect()
    }
}

fn cleanup_title(name: &str) -> String {
    let base = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let cleaned = base.replace(['_', '-'], " ").trim().to_string();
    if cleaned.is_empty() {
        name.to_string()
    } else {
        cleaned
    }
}

/// Проверяет идентификатор приложения webOS.
pub fn valid_app_id(id: &str) -> bool {
    if id.len() < 4 || id.len() > 128 || !id.contains('.') {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && id.split('.').all(|part| {
            !part.is_empty()
                && !part
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(true)
        })
}

/// Генерирует `appinfo.json` для веб-приложения.
pub fn appinfo(spec: &WebAppSpec, main: &str) -> String {
    let value = json!({
        "id": spec.id,
        "version": spec.version,
        "vendor": spec.vendor,
        "type": "web",
        "main": main,
        "title": spec.title,
        "icon": "icon.png",
        "largeIcon": "app-icon-large.png",
        "appDescription": spec.description,
        "iconColor": spec.icon_color,
        "bgColor": "#0b0d12",
        "resolution": "1920x1080",
        "disableScreenTimeout": true,
        "visible": true,
    });
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
}

/// Собирает IPK из готового набора файлов приложения.
pub fn package(spec: &WebAppSpec, mut files: Vec<PackageFile>) -> NovaResult<Vec<u8>> {
    let root = format!("usr/palm/applications/{}", spec.id);
    files.push(PackageFile::new(
        format!("{root}/appinfo.json"),
        appinfo(spec, "index.html").into_bytes(),
    ));
    files.push(PackageFile::new(
        format!("{root}/icon.png"),
        png::generated_icon(80, &spec.id)?,
    ));
    files.push(PackageFile::new(
        format!("{root}/app-icon-large.png"),
        png::generated_icon(130, &spec.id)?,
    ));
    files.push(PackageFile::new(
        format!("usr/palm/packages/{}/packageinfo.json", spec.id),
        json!({ "id": spec.id, "version": spec.version, "app": spec.id })
            .to_string()
            .into_bytes(),
    ));
    let control = ControlFile::new(spec.id.clone(), spec.version.clone(), "all");
    ipk::build(control, files)
}

/// Упаковывает одиночный HTML-файл в приложение webOS.
pub fn from_html(html: &[u8], spec: &WebAppSpec) -> NovaResult<Vec<u8>> {
    package(
        spec,
        vec![PackageFile::new(
            format!("usr/palm/applications/{}/index.html", spec.id),
            html.to_vec(),
        )],
    )
}

/// Упаковывает ZIP-архив с веб-приложением.
///
/// Корень архива определяется автоматически: если `index.html` лежит в подкаталоге
/// (типичный случай `my-app/index.html`), префикс отбрасывается.
pub fn from_zip(bytes: &[u8], spec: &WebAppSpec) -> NovaResult<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| NovaError::Package(format!("ZIP не открыт: {e}")))?;

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| NovaError::Package(format!("ZIP: {e}")))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if name.starts_with("__MACOSX/")
            || name.contains("/.DS_Store")
            || name.ends_with("/.DS_Store")
        {
            continue;
        }
        if file.size() > 200 * 1024 * 1024 {
            return Err(NovaError::Package(format!(
                "элемент {name} больше 200 МБ — не помещается в раздел приложений"
            )));
        }
        let mut data = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut data)
            .map_err(|e| NovaError::Package(format!("чтение {name}: {e}")))?;
        entries.push((name, data));
    }
    if entries.is_empty() {
        return Err(NovaError::Package("ZIP-архив пуст".into()));
    }

    let prefix = common_prefix(&entries);
    let root = format!("usr/palm/applications/{}", spec.id);
    let mut files = Vec::new();
    let mut has_index = false;
    for (name, data) in entries {
        let relative = name.strip_prefix(&prefix).unwrap_or(&name);
        let relative = relative.trim_start_matches('/');
        if relative.is_empty() || relative.contains("..") {
            continue; // защита от zip-slip
        }
        if relative == "index.html" {
            has_index = true;
        }
        files.push(PackageFile::new(format!("{root}/{relative}"), data));
    }
    if !has_index {
        // Нет index.html — создаём страницу-оглавление со ссылками на файлы.
        files.push(PackageFile::new(
            format!("{root}/index.html"),
            index_listing_html(spec, &files).into_bytes(),
        ));
    }
    package(spec, files)
}

/// Общий префикс путей в архиве (например, `my-app/`).
fn common_prefix(entries: &[(String, Vec<u8>)]) -> String {
    let mut prefix: Option<String> = None;
    for (name, _) in entries {
        let Some(index) = name.find('/') else {
            return String::new();
        };
        let candidate = &name[..=index];
        match &prefix {
            Some(current) if current == candidate => {}
            Some(_) => return String::new(),
            None => prefix = Some(candidate.to_string()),
        }
    }
    prefix.unwrap_or_default()
}

fn index_listing_html(spec: &WebAppSpec, files: &[PackageFile]) -> String {
    let root = format!("usr/palm/applications/{}", spec.id);
    let mut items = String::new();
    for file in files {
        if let Some(relative) = file.path.strip_prefix(&format!("{root}/")) {
            if relative.ends_with(".html") {
                items.push_str(&format!("<li><a href=\"{relative}\">{relative}</a></li>\n"));
            }
        }
    }
    if items.is_empty() {
        items = "<li>В архиве не найдено HTML-файлов.</li>".into();
    }
    format!(
        r#"<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  body {{ background:#0b0d12; color:#f5f7fa; font-family:sans-serif; padding:48px; }}
  h1 {{ font-size:36px; margin-bottom:8px; }}
  p {{ color:#98a0ae; }}
  a {{ color:#4c8dff; text-decoration:none; font-size:24px; }}
  li {{ margin:12px 0; }}
</style>
</head>
<body>
<h1>{title}</h1>
<p>В архиве не было index.html — NovaOS создал оглавление.</p>
<ul>
{items}</ul>
</body>
</html>
"#,
        title = spec.title
    )
}

/// Упаковывает каталог приложения.
///
/// Если в каталоге уже есть `appinfo.json`, он используется как есть
/// (нативное приложение сохраняется вместе с двоичным файлом и `lib/`).
pub fn from_directory(path: &Path, spec: &WebAppSpec) -> NovaResult<Vec<u8>> {
    if !path.is_dir() {
        return Err(NovaError::Invalid(format!(
            "{} не является каталогом",
            path.display()
        )));
    }
    let root = format!("usr/palm/applications/{}", spec.id);
    let mut files = Vec::new();
    let mut has_appinfo = false;
    let mut has_index = false;
    collect_dir(
        path,
        path,
        &root,
        &mut files,
        &mut has_appinfo,
        &mut has_index,
    )?;
    if files.is_empty() {
        return Err(NovaError::Package("каталог пуст".into()));
    }
    if !has_index {
        files.push(PackageFile::new(
            format!("{root}/index.html"),
            index_listing_html(spec, &files).into_bytes(),
        ));
    }
    if has_appinfo {
        // Свой манифест важнее сгенерированного: собираем пакет без подмены.
        let control = ControlFile::new(spec.id.clone(), spec.version.clone(), "all");
        files.push(PackageFile::new(
            format!("usr/palm/packages/{}/packageinfo.json", spec.id),
            json!({ "id": spec.id, "version": spec.version, "app": spec.id })
                .to_string()
                .into_bytes(),
        ));
        return ipk::build(control, files);
    }
    package(spec, files)
}

fn collect_dir(
    base: &Path,
    current: &Path,
    root: &str,
    files: &mut Vec<PackageFile>,
    has_appinfo: &mut bool,
    has_index: &mut bool,
) -> NovaResult<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_dir(base, &path, root, files, has_appinfo, has_index)?;
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .map_err(|e| NovaError::Invalid(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let data = std::fs::read(&path)?;
        if relative == "appinfo.json" {
            *has_appinfo = true;
        }
        if relative == "index.html" {
            *has_index = true;
        }
        let executable = is_executable_name(&relative);
        let file = if executable {
            PackageFile::executable(format!("{root}/{relative}"), data)
        } else {
            PackageFile::new(format!("{root}/{relative}"), data)
        };
        files.push(file);
    }
    Ok(())
}

fn is_executable_name(name: &str) -> bool {
    if let Some(last) = name.split('/').last() {
        if !last.contains('.') {
            return true; // файлы без расширения считаем двоичными
        }
        return matches!(last.rsplit('.').next().unwrap_or(""), "sh" | "bin" | "elf");
    }
    false
}

/// Создаёт IPTV-приложение из плейлиста M3U.
pub fn from_playlist(playlist: &str, spec: &WebAppSpec) -> NovaResult<Vec<u8>> {
    let channels = parse_m3u(playlist);
    if channels.is_empty() {
        return Err(NovaError::Package(
            "в плейлисте не найдено ни одного канала".into(),
        ));
    }
    let html = player_html(spec, &channels);
    package(
        spec,
        vec![PackageFile::new(
            format!("usr/palm/applications/{}/index.html", spec.id),
            html.into_bytes(),
        )],
    )
}

/// Канал из плейлиста M3U.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// Название.
    pub name: String,
    /// Адрес потока.
    pub url: String,
    /// Группа.
    pub group: Option<String>,
    /// Логотип.
    pub logo: Option<String>,
}

/// Разбирает плейлист M3U/M3U8.
pub fn parse_m3u(text: &str) -> Vec<Channel> {
    let mut channels = Vec::new();
    let mut pending: Option<(String, Option<String>, Option<String>)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXTINF") {
            let name = line
                .rsplit_once(',')
                .map(|(_, name)| name.trim().to_string())
                .unwrap_or_else(|| "Канал".into());
            let group = attribute(line, "group-title");
            let logo = attribute(line, "tvg-logo");
            pending = Some((name, group, logo));
        } else if line.starts_with('#') {
            continue;
        } else if let Some((name, group, logo)) = pending.take() {
            channels.push(Channel {
                name,
                url: line.to_string(),
                group,
                logo,
            });
        } else {
            channels.push(Channel {
                name: line.to_string(),
                url: line.to_string(),
                group: None,
                logo: None,
            });
        }
    }
    channels
}

fn attribute(line: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=\"");
    let start = line.find(&pattern)? + pattern.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

/// HTML-плеер для сгенерированного IPTV-приложения.
fn player_html(spec: &WebAppSpec, channels: &[Channel]) -> String {
    let list: Vec<serde_json::Value> = channels
        .iter()
        .map(|channel| {
            json!({
                "name": channel.name,
                "url": channel.url,
                "group": channel.group.clone().unwrap_or_else(|| "Без группы".into()),
                "logo": channel.logo.clone().unwrap_or_default(),
            })
        })
        .collect();
    let data = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
    format!(
        r#"<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  html,body {{ width:100%; height:100%; background:#0b0d12; color:#f5f7fa;
    font-family:"Museo Sans","Helvetica Neue",sans-serif; overflow:hidden; }}
  #layout {{ display:flex; height:100%; }}
  #list {{ width:32%; height:100%; overflow-y:auto; border-right:1px solid #1c2029; padding:24px 0; }}
  #player {{ flex:1; display:flex; align-items:center; justify-content:center; background:#000; position:relative; }}
  video {{ width:100%; height:100%; }}
  .ch {{ padding:16px 28px; font-size:20px; color:#c8ceda; }}
  .ch.on {{ background:linear-gradient(90deg,#1b6ef3,#0b0d12); color:#fff; border-left:4px solid #4c8dff; }}
  .grp {{ padding:18px 28px 6px; font-size:14px; letter-spacing:.12em; text-transform:uppercase; color:#6d7481; }}
  #now {{ position:absolute; left:32px; bottom:32px; font-size:26px; text-shadow:0 2px 12px #000; }}
  #hint {{ position:absolute; right:32px; bottom:32px; font-size:16px; color:#8b93a1; }}
</style>
</head>
<body>
<div id="layout">
  <div id="list"></div>
  <div id="player">
    <video id="v" controls="false"></video>
    <div id="now">Выберите канал</div>
    <div id="hint">◀ ▶ выбор · OK воспроизвести · BACK назад</div>
  </div>
</div>
<script>
var CHANNELS = {data};
var index = 0;
var list = document.getElementById('list');
var video = document.getElementById('v');
var now = document.getElementById('now');

function render() {{
  list.innerHTML = '';
  var group = null;
  CHANNELS.forEach(function(ch, i) {{
    if (ch.group !== group) {{
      group = ch.group;
      var header = document.createElement('div');
      header.className = 'grp';
      header.textContent = group;
      list.appendChild(header);
    }}
    var row = document.createElement('div');
    row.className = 'ch' + (i === index ? ' on' : '');
    row.textContent = ch.name;
    list.appendChild(row);
  }});
  var active = list.querySelector('.on');
  if (active) active.scrollIntoView({{ block: 'nearest' }});
}}

function play() {{
  var ch = CHANNELS[index];
  if (!ch) return;
  now.textContent = ch.name;
  video.src = ch.url;
  video.play();
}}

document.addEventListener('keydown', function(e) {{
  switch (e.keyCode) {{
    case 38: index = Math.max(0, index - 1); render(); break;
    case 40: index = Math.min(CHANNELS.length - 1, index + 1); render(); break;
    case 37: index = Math.max(0, index - 10); render(); break;
    case 39: index = Math.min(CHANNELS.length - 1, index + 10); render(); break;
    case 13: play(); break;
    case 461: case 27: if (window.close) window.close(); break;
  }}
  e.preventDefault();
}});

render();
</script>
</body>
</html>
"#,
        title = spec.title,
        data = data
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_and_id_validation() {
        assert_eq!(slugify("Мой Плейлист.m3u"), "m3u");
        assert_eq!(slugify("My App 2"), "my-app-2");
        assert_eq!(slugify("___"), "app");
        assert_eq!(slugify("2048"), "a-2048");
        assert!(valid_app_id("com.novaos.side.my-app"));
        assert!(!valid_app_id("badid"));
        assert!(!valid_app_id("com..bad"));
        assert!(!valid_app_id("com.1bad.id"));
    }

    #[test]
    fn spec_from_name() {
        let spec = WebAppSpec::from_name("Super Player.zip");
        assert_eq!(spec.id, "com.novaos.side.super-player");
        assert_eq!(spec.title, "Super Player");
        assert!(spec.clone().with_id("com.novaos.tv").is_ok());
        assert!(spec.with_id("плохой id").is_err());
    }

    #[test]
    fn html_becomes_installable_ipk() {
        let spec = WebAppSpec::from_name("test.html");
        let ipk = from_html(
            "<!DOCTYPE html><html><body>Привет</body></html>".as_bytes(),
            &spec,
        )
        .unwrap();
        let info = ipk::parse(&ipk).unwrap();
        assert_eq!(info.app_id(), spec.id);
        assert_eq!(info.title(), spec.title);
        assert_eq!(info.app_type(), "web");
        assert!(!info.is_native());
        let paths: Vec<&str> = info.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.iter().any(|p| p.ends_with("index.html")));
        assert!(paths.iter().any(|p| p.ends_with("icon.png")));
        assert!(paths.iter().any(|p| p.ends_with("app-icon-large.png")));
        assert!(paths.iter().any(|p| p.ends_with("packageinfo.json")));
    }

    #[test]
    fn zip_with_subfolder_is_flattened() {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            zip.start_file("my-app/index.html", options).unwrap();
            std::io::Write::write_all(&mut zip, b"<html>ok</html>").unwrap();
            zip.start_file("my-app/app.js", options).unwrap();
            std::io::Write::write_all(&mut zip, b"console.log(1)").unwrap();
            zip.start_file("__MACOSX/junk", options).unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
            zip.finish().unwrap();
        }
        let spec = WebAppSpec::from_name("my-app.zip");
        let ipk = from_zip(&buffer.into_inner(), &spec).unwrap();
        let info = ipk::parse(&ipk).unwrap();
        let paths: Vec<&str> = info.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("/index.html")),
            "{paths:?}"
        );
        assert!(paths.iter().any(|p| p.ends_with("/app.js")));
        assert!(!paths.iter().any(|p| p.contains("__MACOSX")));
        assert!(
            !paths.iter().any(
                |p| p.contains("my-app/index.html") && p.contains("applications/my-app/my-app")
            ),
            "префикс каталога должен отбрасываться"
        );
    }

    #[test]
    fn playlist_becomes_player_app() {
        let m3u = "#EXTM3U\n#EXTINF:-1 group-title=\"Новости\" tvg-logo=\"http://x/1.png\",Первый канал\nhttp://example.com/1.m3u8\n#EXTINF:-1 group-title=\"Спорт\",Матч\nhttp://example.com/2.m3u8\n";
        let channels = parse_m3u(m3u);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Первый канал");
        assert_eq!(channels[0].group.as_deref(), Some("Новости"));
        assert_eq!(channels[0].logo.as_deref(), Some("http://x/1.png"));
        assert_eq!(channels[1].url, "http://example.com/2.m3u8");

        let spec = WebAppSpec::from_name("iptv.m3u").with_title("Мой IPTV");
        let ipk = from_playlist(m3u, &spec).unwrap();
        let info = ipk::parse(&ipk).unwrap();
        assert_eq!(info.title(), "Мой IPTV");
        assert!(from_playlist("#EXTM3U\n", &spec).is_err());
    }

    #[test]
    fn directory_packaging() {
        let dir = std::env::temp_dir().join(format!("nova-webapp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<html>dir</html>").unwrap();
        std::fs::write(dir.join("assets/style.css"), "body{}").unwrap();
        let spec = WebAppSpec::from_name("dir-app");
        let ipk = from_directory(&dir, &spec).unwrap();
        let info = ipk::parse(&ipk).unwrap();
        let paths: Vec<&str> = info.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.iter().any(|p| p.ends_with("assets/style.css")));
        assert_eq!(info.app_id(), spec.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn executable_bit_is_preserved() {
        assert!(is_executable_name("usr/bin/nova"));
        assert!(is_executable_name("run.sh"));
        assert!(!is_executable_name("index.html"));
    }
}
