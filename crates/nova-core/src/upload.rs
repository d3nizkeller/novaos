//! Локальный HTTP-приёмник файлов («установка с телефона»).
//!
//! Сценарий: на экране «Установка» NovaOS поднимает крошечный HTTP-сервер в
//! локальной сети и рисует QR-код с адресом `http://<IP-телевизора>:7788/upload`.
//! Пользователь наводит камеру телефона, открывает страницу, выбирает `.ipk`
//! (или `.apk`, `.zip`, `.html`, `.m3u`) — файл уходит прямо на телевизор и
//! сразу попадает в конвейер установки [`crate::install`].
//!
//! Сервер написан только на `std`, поэтому собирается и под
//! `armv7-unknown-linux-gnueabi` без дополнительных зависимостей.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{NovaError, NovaResult};

/// Порт по умолчанию.
pub const DEFAULT_PORT: u16 = 7788;
/// Ограничение размера принимаемого файла (512 МБ) — защита от исчерпания памяти.
pub const MAX_UPLOAD: u64 = 512 * 1024 * 1024;
/// Ограничение размера заголовков запроса.
const MAX_HEADERS: usize = 64 * 1024;

/// Принятый файл.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    /// Имя файла (очищено от пути).
    pub filename: String,
    /// Содержимое.
    pub bytes: Vec<u8>,
    /// Куда сохранено на устройстве (если каталог доступен для записи).
    pub saved_to: Option<PathBuf>,
}

impl Upload {
    /// Размер в байтах.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Пуст ли файл.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// HTTP-приёмник файлов.
#[derive(Debug)]
pub struct UploadServer {
    listener: TcpListener,
    dir: PathBuf,
    stop: Arc<AtomicBool>,
}

impl UploadServer {
    /// Запускает сервер. `port = 0` — выбрать свободный порт (тесты, несколько копий).
    pub fn start(dir: impl Into<PathBuf>, port: u16) -> NovaResult<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .map_err(|e| NovaError::Io(io_error(format!("каталог {dir:?}: {e}"))))?;
        let listener = TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| NovaError::Io(io_error(format!("не удалось занять порт {port}: {e}"))))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| NovaError::Io(io_error(format!("неблокирующий режим: {e}"))))?;
        Ok(Self {
            listener,
            dir,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Фактический порт.
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(DEFAULT_PORT)
    }

    /// Каталог, куда сохраняются принятые файлы.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Адрес страницы для QR-кода.
    pub fn url(&self, ip: Option<&str>) -> String {
        let fallback = local_ip();
        let ip = match ip {
            Some(value) if !value.is_empty() => Some(value.to_string()),
            _ => fallback,
        };
        crate::qr::upload_url(ip.as_deref(), self.port())
    }

    /// Флаг остановки — можно передать в другой поток.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// Просит сервер завершить работу.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Остановлен ли сервер.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Обрабатывает одно подключение, если оно появилось в течение `timeout`.
    ///
    /// `Ok(None)` — соединений не было (или сервер остановлен),
    /// `Ok(Some(upload))` — файл принят и сохранён.
    pub fn handle_next(&self, timeout: Duration) -> NovaResult<Option<Upload>> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.stopped() {
                return Ok(None);
            }
            match self.listener.accept() {
                Ok((stream, _peer)) => {
                    if let Some(upload) = self.serve(stream)? {
                        return Ok(Some(upload));
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(4));
                }
                Err(e) => return Err(NovaError::Io(io_error(format!("приём соединения: {e}")))),
            }
        }
    }

    /// Цикл приёма файлов: вызывает `on_upload` для каждого принятого файла.
    ///
    /// Возвращает число принятых файлов. Цикл завершается, когда `on_upload`
    /// вернёт `false` либо после [`UploadServer::stop`].
    pub fn run<F>(&self, mut on_upload: F) -> NovaResult<usize>
    where
        F: FnMut(Upload) -> bool,
    {
        let mut count = 0;
        while !self.stopped() {
            if let Some(upload) = self.handle_next(Duration::from_millis(400))? {
                count += 1;
                if !on_upload(upload) {
                    break;
                }
            }
        }
        Ok(count)
    }

    /// Запускает приём файлов в фоновом потоке.
    ///
    /// Принятые файлы приходят в [`UploadHandle`]; сервер останавливается
    /// вызовом [`UploadHandle::stop`] или при удалении ручки.
    pub fn spawn(self) -> UploadHandle {
        let port = self.port();
        let stop = self.stop.clone();
        let thread_stop = stop.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("nova-upload".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    match self.handle_next(Duration::from_millis(250)) {
                        Ok(Some(upload)) => {
                            if tx.send(upload).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
            .expect("не удалось запустить поток приёма файлов");
        UploadHandle {
            port,
            stop,
            rx,
            join: Some(join),
        }
    }

    /// Обслуживает одно HTTP-соединение.
    fn serve(&self, mut stream: TcpStream) -> NovaResult<Option<Upload>> {
        stream.set_read_timeout(Some(Duration::from_secs(180))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(30))).ok();

        let (head, leftover) = match read_head(&mut stream)? {
            Some(parsed) => parsed,
            None => return Ok(None),
        };
        let method = head.method.to_ascii_uppercase();
        let head_only = method == "HEAD";

        if method == "GET" || head_only {
            let body = if head.path.starts_with("/ok") {
                ok_page()
            } else {
                index_page(self.port())
            };
            respond(
                &mut stream,
                200,
                "OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                head_only,
            )?;
            return Ok(None);
        }
        if method != "POST" {
            respond(
                &mut stream,
                405,
                "Method Not Allowed",
                "text/plain; charset=utf-8",
                b"NovaOS: method not allowed",
                false,
            )?;
            return Ok(None);
        }
        let length = head.content_length.unwrap_or(0);
        if length > MAX_UPLOAD {
            respond(
                &mut stream,
                413,
                "Payload Too Large",
                "text/plain; charset=utf-8",
                b"NovaOS: file too large",
                false,
            )?;
            return Ok(None);
        }

        let body = read_body(&mut stream, length, leftover)?;
        let (raw_name, bytes) = match head.boundary() {
            Some(boundary) => match parse_multipart(&body, &boundary) {
                Some(part) => part,
                None => {
                    respond(
                        &mut stream,
                        400,
                        "Bad Request",
                        "text/plain; charset=utf-8",
                        b"NovaOS: multipart not understood",
                        false,
                    )?;
                    return Ok(None);
                }
            },
            None => {
                let name = head
                    .header("x-filename")
                    .or_else(|| head.disposition_filename())
                    .or_else(|| filename_from_path(&head.path))
                    .unwrap_or_else(|| "upload.ipk".to_string());
                (name, body)
            }
        };
        if bytes.is_empty() {
            respond(
                &mut stream,
                400,
                "Bad Request",
                "text/plain; charset=utf-8",
                b"NovaOS: empty file",
                false,
            )?;
            return Ok(None);
        }

        let filename = sanitize_filename(&raw_name);
        let saved_to = save(&self.dir, &filename, &bytes)?;
        let stored_name = saved_to
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| filename.clone());
        respond(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            ok_page().as_bytes(),
            false,
        )?;
        Ok(Some(Upload {
            filename: stored_name,
            bytes,
            saved_to,
        }))
    }
}

/// Ручка фонового приёмника файлов.
#[derive(Debug)]
pub struct UploadHandle {
    port: u16,
    stop: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<Upload>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl UploadHandle {
    /// Порт сервера.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Адрес страницы для QR-кода.
    pub fn url(&self, ip: Option<&str>) -> String {
        let fallback = local_ip();
        let ip = match ip {
            Some(value) if !value.is_empty() => Some(value.to_string()),
            _ => fallback,
        };
        crate::qr::upload_url(ip.as_deref(), self.port)
    }

    /// Файл, если он уже пришёл (без ожидания).
    pub fn try_recv(&self) -> Option<Upload> {
        self.rx.try_recv().ok()
    }

    /// Ждёт файл не дольше `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Upload> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Останавливает сервер.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Работает ли сервер.
    pub fn running(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }
}

impl Drop for UploadHandle {
    fn drop(&mut self) {
        self.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Разобранные заголовки запроса.
#[derive(Debug, Clone)]
struct RequestHead {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    content_length: Option<u64>,
}

impl RequestHead {
    fn header(&self, name: &str) -> Option<String> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    }

    fn content_type(&self) -> String {
        self.header("content-type").unwrap_or_default()
    }

    /// Граница `multipart/form-data`, если запрос именно такой.
    fn boundary(&self) -> Option<String> {
        let ct = self.content_type();
        if !ct.to_ascii_lowercase().starts_with("multipart/form-data") {
            return None;
        }
        ct.split(';').find_map(|part| {
            let part = part.trim();
            part.to_ascii_lowercase()
                .starts_with("boundary=")
                .then(|| part["boundary=".len()..].trim_matches('"').to_string())
        })
    }

    /// Имя файла из `Content-Disposition`.
    fn disposition_filename(&self) -> Option<String> {
        parse_filename_param(&self.header("content-disposition")?)
    }
}

/// Читает заголовки; возвращает их и уже прочитанный хвост тела.
fn read_head(stream: &mut TcpStream) -> NovaResult<Option<(RequestHead, Vec<u8>)>> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let split = loop {
        if let Some(at) = find_subslice(&buf, b"\r\n\r\n") {
            break at;
        }
        if buf.len() > MAX_HEADERS {
            return Ok(None);
        }
        let read = stream
            .read(&mut chunk)
            .map_err(|e| NovaError::Io(io_error(format!("чтение запроса: {e}"))))?;
        if read == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..read]);
    };

    let text = String::from_utf8_lossy(&buf[..split]).into_owned();
    let mut lines = text.lines();
    let request_line = lines.next().unwrap_or("").to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut headers = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let mut head = RequestHead {
        method,
        path,
        headers,
        content_length: None,
    };
    head.content_length = head.header("content-length").and_then(|v| v.parse().ok());
    let leftover = buf.split_off(split + 4);
    Ok(Some((head, leftover)))
}

/// Дочитывает тело заданной длины.
fn read_body(stream: &mut TcpStream, length: u64, mut body: Vec<u8>) -> NovaResult<Vec<u8>> {
    if length == 0 {
        body.clear();
        return Ok(body);
    }
    body.truncate(length.min(body.len() as u64) as usize);
    body.reserve((length - body.len() as u64) as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    while (body.len() as u64) < length {
        let read = stream
            .read(&mut chunk)
            .map_err(|e| NovaError::Io(io_error(format!("чтение тела запроса: {e}"))))?;
        if read == 0 {
            break;
        }
        let room = (length - body.len() as u64) as usize;
        body.extend_from_slice(&chunk[..read.min(room)]);
        if body.len() as u64 > MAX_UPLOAD {
            return Err(NovaError::Invalid("файл больше 512 МБ".into()));
        }
    }
    Ok(body)
}

fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> NovaResult<()> {
    let mut out = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    )
    .into_bytes();
    if !head_only {
        out.extend_from_slice(body);
    }
    stream
        .write_all(&out)
        .map_err(|e| NovaError::Io(io_error(format!("ответ клиенту: {e}"))))?;
    stream.flush().ok();
    Ok(())
}

fn io_error(message: String) -> std::io::Error {
    std::io::Error::other(message)
}

/// Ищет подпоследовательность байтов.
pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Разбирает `multipart/form-data`: имя файла и байты первой части с файлом.
pub fn parse_multipart(body: &[u8], boundary: &str) -> Option<(String, Vec<u8>)> {
    let delimiter = format!("--{boundary}");
    let mut cursor = 0;
    while let Some(start) = find_subslice(&body[cursor..], delimiter.as_bytes()) {
        let part_start = cursor + start + delimiter.len();
        let rest = &body[part_start..];
        if rest.starts_with(b"--") {
            break;
        }
        let rest = rest.strip_prefix(b"\r\n").unwrap_or(rest);
        let header_end = find_subslice(rest, b"\r\n\r\n")?;
        let headers = String::from_utf8_lossy(&rest[..header_end]).into_owned();
        let data = &rest[header_end + 4..];
        let end = find_subslice(data, format!("\r\n{delimiter}").as_bytes()).unwrap_or(data.len());
        let filename = headers.lines().find_map(|line| {
            line.to_ascii_lowercase()
                .starts_with("content-disposition")
                .then(|| parse_filename_param(line))
                .flatten()
        });
        if let Some(name) = filename {
            return Some((name, data[..end].to_vec()));
        }
        cursor = part_start + end;
    }
    None
}

/// Достаёт `filename="..."` из строки заголовка.
fn parse_filename_param(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let key = "filename=";
    let at = lower.find(key)? + key.len();
    let rest = &text[at..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest.find([';', '\r', '\n']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

/// Имя файла из пути запроса (`/upload/luma.ipk`).
fn filename_from_path(path: &str) -> Option<String> {
    let trimmed = path.split(['?', '#']).next().unwrap_or(path);
    let name = trimmed.rsplit('/').next().unwrap_or("");
    (!name.is_empty() && name != "upload").then(|| name.to_string())
}

/// Убирает из имени файла путь и символы, недопустимые в файловой системе.
pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            // Управляющие символы и запрещённые в именах файлов.
            '\0'..='\u{1f}' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).trim().to_string();
    if cleaned.is_empty() {
        "upload.ipk".to_string()
    } else {
        cleaned
    }
}

/// Сохраняет файл в каталог, подбирая уникальное имя (`app (1).ipk`).
pub fn save(dir: &Path, filename: &str, bytes: &[u8]) -> NovaResult<Option<PathBuf>> {
    if std::fs::create_dir_all(dir).is_err() {
        return Ok(None);
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (filename.to_string(), String::new()),
    };
    let mut candidate = dir.join(format!("{stem}{ext}"));
    let mut counter = 1u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({counter}){ext}"));
        counter += 1;
        if counter > 999 {
            return Ok(None);
        }
    }
    match std::fs::write(&candidate, bytes) {
        Ok(()) => Ok(Some(candidate)),
        Err(_) => Ok(None),
    }
}

/// Локальный IP-адрес телевизора.
///
/// Пакеты не отправляются: `connect` на UDP лишь выбирает интерфейс маршрута.
pub fn local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip().to_string();
    (!ip.starts_with("127.")).then_some(ip)
}

/// Страница выбора файла — открывается на телефоне по QR-коду.
pub fn index_page(port: u16) -> String {
    format!(
        r##"<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>NovaOS — установка приложения</title>
<style>
:root {{ color-scheme: dark; }}
body {{ margin:0; min-height:100vh; display:flex; align-items:center; justify-content:center;
       background:#0b0d12; color:#f5f7fa; font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }}
main {{ width:min(92vw,420px); padding:28px; border-radius:20px; background:#171a21; border:1px solid #262b36; }}
h1 {{ font-size:22px; margin:0 0 8px; }}
p {{ margin:0 0 20px; color:#a4adbb; }}
label {{ display:block; padding:16px; border-radius:14px; border:1px dashed #4c8dff;
        text-align:center; cursor:pointer; color:#cfe0ff; }}
input {{ display:none; }}
progress {{ width:100%; height:8px; margin-top:18px; display:none; }}
small {{ display:block; margin-top:16px; color:#6c7482; }}
</style>
</head>
<body>
<main>
<h1>NovaOS</h1>
<p>Выберите файл приложения — он отправится на телевизор и установится автоматически.</p>
<form id="f" method="post" action="/upload" enctype="multipart/form-data">
  <label for="file" id="pick">Выбрать файл (.ipk, .apk, .zip, .html, .m3u)</label>
  <input id="file" name="file" type="file" accept=".ipk,.apk,.zip,.html,.htm,.m3u,.m3u8,application/octet-stream">
  <progress id="bar" max="100" value="0"></progress>
</form>
<small>Порт {port} · файл не покидает вашу локальную сеть</small>
<script>
var file = document.getElementById('file');
var pick = document.getElementById('pick');
var bar = document.getElementById('bar');
file.addEventListener('change', function () {{
  if (!file.files.length) return;
  pick.textContent = file.files[0].name;
  var data = new FormData();
  data.append('file', file.files[0]);
  var xhr = new XMLHttpRequest();
  bar.style.display = 'block';
  xhr.upload.onprogress = function (e) {{
    if (e.lengthComputable) bar.value = Math.round(e.loaded / e.total * 100);
  }};
  xhr.onload = function () {{ window.location = '/ok'; }};
  xhr.onerror = function () {{ pick.textContent = 'Ошибка отправки — попробуйте ещё раз'; }};
  xhr.open('POST', '/upload');
  xhr.send(data);
}});
</script>
</main>
</body>
</html>
"##
    )
}

/// Страница «файл получен».
pub fn ok_page() -> String {
    concat!(
        "<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>NovaOS — файл получен</title>",
        "<style>body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;",
        "background:#0b0d12;color:#f5f7fa;font:16px/1.5 system-ui,sans-serif;text-align:center}",
        "main{padding:32px}h1{font-size:44px;margin:0 0 12px}</style></head>",
        "<body><main><h1>✓</h1><p>Файл получен. Посмотрите на экран телевизора —<br>там идёт установка.</p></main></body></html>"
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("nova-upload-{tag}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn send(port: u16, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("подключение");
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        stream.write_all(request).expect("запись запроса");
        let mut out = Vec::new();
        let _ = stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn post_request(headers: &str, payload: &[u8]) -> Vec<u8> {
        let mut request = format!(
            "POST /upload HTTP/1.1\r\nHost: tv\r\n{headers}Content-Length: {}\r\n\r\n",
            payload.len()
        )
        .into_bytes();
        request.extend_from_slice(payload);
        request
    }

    #[test]
    fn serves_index_page() {
        let dir = temp_dir("index");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let handle = server.spawn();
        let response = send(handle.port(), b"GET / HTTP/1.1\r\nHost: tv\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("NovaOS"));
        assert!(response.contains("type=\"file\""));
        assert!(handle.url(Some("192.168.1.20")).contains("192.168.1.20"));
        assert!(handle.running());
        handle.stop();
        assert!(!handle.running());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn receives_raw_post_and_saves_file() {
        let dir = temp_dir("raw");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let handle = server.spawn();
        let payload = b"!<arch>\ndebian-binary   0           0    0     100644  4         `\n2.0\n";
        let request = post_request("X-Filename: luma.ipk\r\n", payload);

        let port = handle.port();
        let client = std::thread::spawn(move || send(port, &request));
        let upload = handle
            .recv_timeout(Duration::from_secs(15))
            .expect("файл принят");
        let response = client.join().unwrap();
        assert!(response.contains("200 OK"), "{response}");
        assert!(response.contains("Файл получен"), "{response}");

        assert_eq!(upload.filename, "luma.ipk");
        assert_eq!(upload.bytes, payload);
        assert_eq!(upload.len(), payload.len());
        let saved = upload.saved_to.expect("файл сохранён");
        assert_eq!(saved.parent().unwrap(), dir);
        assert_eq!(std::fs::read(&saved).unwrap(), payload);
        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn receives_multipart_post_and_sanitizes_name() {
        let dir = temp_dir("multipart");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let handle = server.spawn();
        let boundary = "----NovaBoundary42";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../../evil name.ipk\"\r\nContent-Type: application/octet-stream\r\n\r\nIPKDATA\r\n--{boundary}--\r\n"
        )
        .into_bytes();
        let request = post_request(
            &format!("Content-Type: multipart/form-data; boundary={boundary}\r\n"),
            &body,
        );

        let port = handle.port();
        let client = std::thread::spawn(move || send(port, &request));
        let upload = handle
            .recv_timeout(Duration::from_secs(15))
            .expect("файл принят");
        let _ = client.join().unwrap();

        assert_eq!(upload.bytes, b"IPKDATA");
        assert!(!upload.filename.contains('/'), "{}", upload.filename);
        assert!(!upload.filename.contains(".."), "{}", upload.filename);
        assert!(upload.saved_to.is_some());
        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_names_get_suffix() {
        let dir = temp_dir("dup");
        let first = save(&dir, "app.ipk", b"1").unwrap().unwrap();
        let second = save(&dir, "app.ipk", b"22").unwrap().unwrap();
        assert_ne!(first, second);
        assert!(
            second.to_string_lossy().contains("app (1).ipk"),
            "{second:?}"
        );
        assert_eq!(std::fs::read(&second).unwrap(), b"22");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filename_sanitized() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\temp\\My App.ipk"), "My App.ipk");
        assert_eq!(sanitize_filename("luma.ipk"), "luma.ipk");
        assert_eq!(sanitize_filename(""), "upload.ipk");
        assert_eq!(sanitize_filename("..."), "upload.ipk");
        assert_eq!(
            filename_from_path("/upload/luma.ipk").as_deref(),
            Some("luma.ipk")
        );
        assert_eq!(filename_from_path("/upload"), None);
        assert_eq!(find_subslice(b"hello world", b"o w"), Some(4));
        assert_eq!(find_subslice(b"ab", b"abc"), None);
    }

    #[test]
    fn run_loop_stops_on_flag() {
        let dir = temp_dir("loop");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let flag = server.stop_flag();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            flag.store(true, Ordering::SeqCst);
        });
        let count = server.run(|_upload| true).unwrap();
        assert_eq!(count, 0);
        stopper.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_loop_stops_on_callback() {
        let dir = temp_dir("callback");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let request = post_request("X-Filename: one.bin\r\n", b"DATA");
        let port = server.port();
        let client = std::thread::spawn(move || send(port, &request));
        let count = server
            .run(|upload| {
                assert_eq!(upload.filename, "one.bin");
                false
            })
            .unwrap();
        assert_eq!(count, 1);
        let _ = client.join().unwrap();
        server.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_oversized_and_wrong_method() {
        let dir = temp_dir("reject");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let handle = server.spawn();
        let response = send(
            handle.port(),
            b"PUT /upload HTTP/1.1\r\nHost: tv\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 405"), "{response}");
        let big = format!(
            "POST /upload HTTP/1.1\r\nHost: tv\r\nContent-Length: {}\r\n\r\n",
            MAX_UPLOAD + 1
        );
        let response = send(handle.port(), big.as_bytes());
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        assert!(handle.try_recv().is_none());
        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_upload_is_rejected() {
        let dir = temp_dir("empty");
        let server = UploadServer::start(dir.clone(), 0).unwrap();
        let handle = server.spawn();
        let request = post_request("X-Filename: empty.ipk\r\n", b"");
        let response = send(handle.port(), &request);
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(handle.try_recv().is_none());
        handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pages_are_russian_and_self_contained() {
        let page = index_page(DEFAULT_PORT);
        assert!(page.contains("Выбрать файл"));
        assert!(page.contains(&DEFAULT_PORT.to_string()));
        assert!(
            !page.contains("http://") && !page.contains("https://") && !page.contains("cdn."),
            "страница не должна тянуть внешние ресурсы"
        );
        assert!(ok_page().contains("Файл получен"));
        assert!(local_ip().map_or(true, |ip| !ip.starts_with("127.")));
    }
}
