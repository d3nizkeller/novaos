//! Вызовы шины Luna — способ, которым приложения webOS общаются с системой.
//!
//! Нативное приложение NovaOS не линкуется с `libluna-service` (это потребовало бы
//! отдельной сборки под soft-float и регистрации сервиса), а вызывает штатные
//! утилиты `luna-send` / `luna-send-pub`. Клиент сам подбирает рабочую шину:
//! сначала публичная (`luna://`), затем приватная (`palm://`), а для операций,
//! требующих root, — сервис Homebrew Channel.
//!
//! Проверенные на реальных устройствах точки:
//!
//! | Задача | URI |
//! |---|---|
//! | Список приложений | `com.webos.applicationManager/dev/listApps`, `.../listApps` |
//! | Запуск | `com.webos.applicationManager/launch` |
//! | Закрытие | `com.webos.applicationManager/close` |
//! | Установка IPK | `com.webos.appInstallService/dev/install` |
//! | Удаление | `com.webos.appInstallService/dev/remove` |
//! | Root-команда | `org.webosbrew.hbchannel.service/exec` |
//! | Сведения о системе | `com.webos.service.systemservice/getSystemInfo` |

use std::cell::RefCell;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::app::WebOsAppInfo;
use crate::error::{NovaError, NovaResult};

/// Шина Luna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LunaBus {
    /// Публичная (`luna-send-pub`, схема `luna://`).
    Public,
    /// Приватная (`luna-send`, схема `palm://`).
    Private,
}

impl LunaBus {
    /// Схема URI для шины.
    pub fn scheme(&self) -> &'static str {
        match self {
            LunaBus::Public => "luna",
            LunaBus::Private => "palm",
        }
    }

    /// Исполняемый файл.
    pub fn binary(&self) -> &'static str {
        match self {
            LunaBus::Public => "luna-send-pub",
            LunaBus::Private => "luna-send",
        }
    }
}

/// Заглушка-обработчик для вызовов без подписки.
type NoOp = fn(&Value) -> bool;

/// Результат вызова Luna.
#[derive(Debug, Clone)]
pub struct LunaResponse {
    /// Все полученные ответы (для подписок их несколько).
    pub payloads: Vec<Value>,
    /// Использованная шина.
    pub bus: LunaBus,
}

impl LunaResponse {
    /// Последний ответ.
    pub fn last(&self) -> Option<&Value> {
        self.payloads.last()
    }

    /// Первый ответ без `returnValue: false`.
    pub fn ok_payload(&self) -> Option<&Value> {
        self.payloads.iter().find(|p| {
            p.get("returnValue")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
    }

    /// Текст ошибки из ответа Luna (`errorText`, `details.reason`).
    pub fn error_text(&self) -> Option<String> {
        for payload in &self.payloads {
            if payload.get("returnValue").and_then(Value::as_bool) == Some(false) {
                let code = payload
                    .get("errorCode")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1);
                let text = payload
                    .get("errorText")
                    .and_then(Value::as_str)
                    .unwrap_or("неизвестная ошибка");
                return Some(format!("[{code}] {text}"));
            }
            if let Some(details) = payload.get("details") {
                if let Some(reason) = details.get("reason").and_then(Value::as_str) {
                    if details.get("errorCode").is_some() {
                        return Some(reason.to_string());
                    }
                }
            }
        }
        None
    }
}

/// Клиент шины Luna.
#[derive(Debug)]
pub struct LunaClient {
    /// Предпочтительная шина (подбирается автоматически).
    preferred: RefCell<Option<LunaBus>>,
    /// Режим «без железа»: все вызовы возвращают ошибку вместо запуска утилит.
    offline: bool,
    /// Таймаут одного вызова.
    pub timeout: Duration,
}

impl Default for LunaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LunaClient {
    /// Клиент, который ищет `luna-send` на устройстве.
    pub fn new() -> Self {
        Self {
            preferred: RefCell::new(None),
            offline: false,
            timeout: Duration::from_secs(60),
        }
    }

    /// Клиент для тестов и desktop-превью: не выполняет системных вызовов.
    pub fn offline() -> Self {
        Self {
            preferred: RefCell::new(None),
            offline: true,
            timeout: Duration::from_secs(5),
        }
    }

    /// Доступна ли шина Luna (есть ли утилиты в системе).
    pub fn available(&self) -> bool {
        if self.offline {
            return false;
        }
        std::path::Path::new("/usr/bin/luna-send").exists()
            || std::path::Path::new("/usr/bin/luna-send-pub").exists()
    }

    fn candidates(&self) -> Vec<LunaBus> {
        match self.preferred.borrow().as_ref() {
            Some(bus) => vec![*bus],
            None => vec![LunaBus::Public, LunaBus::Private],
        }
    }

    /// Полный URI для шины: `luna://service/method` или `palm://service/method`.
    pub fn uri(bus: LunaBus, service_method: &str) -> String {
        let trimmed = service_method
            .trim_start_matches("luna://")
            .trim_start_matches("palm://");
        format!("{}://{trimmed}", bus.scheme())
    }

    /// Выполняет вызов, ожидая `responses` ответов (для подписок — больше одного).
    pub fn call(
        &self,
        service_method: &str,
        payload: &Value,
        responses: usize,
    ) -> NovaResult<LunaResponse> {
        if self.offline {
            return Err(NovaError::Unsupported(
                "шина Luna недоступна в этом режиме (offline)".into(),
            ));
        }
        let mut last_error = None;
        for bus in self.candidates() {
            match self.run::<NoOp>(bus, service_method, payload, responses, None) {
                Ok(response) => {
                    *self.preferred.borrow_mut() = Some(bus);
                    return Ok(response);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| NovaError::Luna("нет доступной шины Luna".into())))
    }

    /// Выполняет вызов с подпиской и обработчиком каждого ответа.
    ///
    /// Обработчик возвращает `true`, чтобы продолжить чтение, `false` — чтобы прервать.
    pub fn subscribe<F>(
        &self,
        service_method: &str,
        payload: &Value,
        mut on_payload: F,
    ) -> NovaResult<LunaResponse>
    where
        F: FnMut(&Value) -> bool,
    {
        if self.offline {
            return Err(NovaError::Unsupported(
                "шина Luna недоступна (offline)".into(),
            ));
        }
        let bus = self.preferred.borrow().unwrap_or(LunaBus::Public);
        let response = self.run(bus, service_method, payload, 64, Some(&mut on_payload))?;
        *self.preferred.borrow_mut() = Some(bus);
        Ok(response)
    }

    fn run<F>(
        &self,
        bus: LunaBus,
        service_method: &str,
        payload: &Value,
        responses: usize,
        mut on_payload: Option<&mut F>,
    ) -> NovaResult<LunaResponse>
    where
        F: FnMut(&Value) -> bool,
    {
        use std::io::{BufRead, BufReader};

        let uri = Self::uri(bus, service_method);
        let body = serde_json::to_string(payload).unwrap_or_else(|_| "{}".into());
        let mut child = Command::new(bus.binary())
            .arg("-n")
            .arg(responses.max(1).to_string())
            .arg(&uri)
            .arg(&body)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| NovaError::Luna(format!("не удалось запустить {}: {e}", bus.binary())))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NovaError::Luna("нет stdout у luna-send".into()))?;
        let reader = BufReader::new(stdout);
        let mut payloads = Vec::new();
        let mut stop = false;
        for line in reader.lines() {
            let line =
                line.map_err(|e| NovaError::Luna(format!("чтение ответа luna-send: {e}")))?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                let keep_going = match &mut on_payload {
                    Some(handler) => handler(&value),
                    None => true,
                };
                payloads.push(value);
                if !keep_going {
                    stop = true;
                    break;
                }
            }
        }
        if stop {
            let _ = child.kill();
        }
        let status = child
            .wait()
            .map_err(|e| NovaError::Luna(format!("ожидание luna-send: {e}")))?;
        if payloads.is_empty() {
            let stderr = child
                .stderr
                .take()
                .map(|mut err| {
                    let mut text = String::new();
                    use std::io::Read;
                    let _ = err.read_to_string(&mut text);
                    text.trim().to_string()
                })
                .unwrap_or_default();
            return Err(NovaError::Luna(format!(
                "пустой ответ от {uri} (код {}){}",
                status.code().unwrap_or(-1),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            )));
        }
        Ok(LunaResponse { payloads, bus })
    }

    // --- Типовые операции -------------------------------------------------

    /// Список установленных приложений.
    ///
    /// Сначала пробуем `dev/listApps` (он отдаёт и скрытые приложения, и `folderPath`),
    /// затем обычный `listApps`.
    pub fn list_apps(&self) -> NovaResult<Vec<WebOsAppInfo>> {
        for endpoint in [
            "com.webos.applicationManager/dev/listApps",
            "com.webos.applicationManager/listApps",
            "com.palm.applicationManager/listApps",
        ] {
            if let Ok(response) = self.call(endpoint, &json!({}), 1) {
                if let Some(apps) = response
                    .payloads
                    .iter()
                    .find_map(|p| p.get("apps").and_then(Value::as_array))
                {
                    let parsed: Vec<WebOsAppInfo> = apps
                        .iter()
                        .filter_map(|value| serde_json::from_value(value.clone()).ok())
                        .collect();
                    if !parsed.is_empty() {
                        return Ok(parsed);
                    }
                }
            }
        }
        Err(NovaError::Luna(
            "не удалось получить список приложений".into(),
        ))
    }

    /// Запускает приложение с параметрами.
    pub fn launch(&self, id: &str, params: &Value) -> NovaResult<()> {
        let response = self.call(
            "com.webos.applicationManager/launch",
            &json!({ "id": id, "params": params }),
            1,
        )?;
        match response.error_text() {
            Some(error) => Err(NovaError::Luna(error)),
            None => Ok(()),
        }
    }

    /// Закрывает приложение.
    pub fn close(&self, id: &str) -> NovaResult<()> {
        let _ = self.call(
            "com.webos.applicationManager/close",
            &json!({ "id": id }),
            1,
        )?;
        Ok(())
    }

    /// Идентификатор приложения на переднем плане.
    pub fn foreground_app(&self) -> Option<String> {
        let response = self
            .call(
                "com.webos.applicationManager/getForegroundAppInfo",
                &json!({}),
                1,
            )
            .ok()?;
        response
            .payloads
            .iter()
            .find_map(|p| p.get("appId").and_then(Value::as_str))
            .map(str::to_string)
    }

    /// Сведения о системе (модель, версия прошивки).
    pub fn system_info(&self) -> Option<Value> {
        let response = self
            .call(
                "com.webos.service.systemservice/getSystemInfo",
                &json!({}),
                1,
            )
            .ok()?;
        response.ok_payload().cloned()
    }

    /// Выполняет команду от имени root через сервис Homebrew Channel.
    pub fn exec_root(&self, command: &str) -> NovaResult<String> {
        let response = self.call(
            "org.webosbrew.hbchannel.service/exec",
            &json!({ "command": command, "timeout": 60_000 }),
            1,
        )?;
        let payload = response.ok_payload().ok_or_else(|| {
            NovaError::Luna(
                response
                    .error_text()
                    .unwrap_or_else(|| "exec отклонён".into()),
            )
        })?;
        if let Some(error) = payload
            .get("error")
            .and_then(Value::as_str)
            .filter(|e| !e.is_empty())
        {
            return Err(NovaError::Permission(error.to_string()));
        }
        Ok(payload
            .get("stdoutString")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// Установлен ли Homebrew Channel (и, значит, доступен root-exec).
    pub fn homebrew_channel_present(&self) -> bool {
        std::path::Path::new("/media/developer/apps/usr/palm/applications/org.webosbrew.hbchannel")
            .exists()
            || std::path::Path::new(
                "/media/cryptofs/apps/usr/palm/applications/org.webosbrew.hbchannel",
            )
            .exists()
    }

    /// Устанавливает IPK через `appInstallService/dev/install` с прогрессом.
    ///
    /// `statusValue == 30` означает успех (значение совпадает с реализацией
    /// Homebrew Channel), остальные коды — промежуточные стадии установки.
    pub fn install_ipk<F>(&self, ipk_path: &str, mut on_status: F) -> NovaResult<String>
    where
        F: FnMut(&str, Option<i64>),
    {
        let payload = json!({
            "id": "novaos",
            "ipkUrl": ipk_path,
            "subscribe": true,
        });
        let mut package_id = None;
        let response = self.subscribe(
            "com.webos.appInstallService/dev/install",
            &payload,
            |value| {
                let status = value.get("statusValue").and_then(Value::as_i64);
                let text = value
                    .get("details")
                    .and_then(|d| d.get("reason").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string();
                on_status(&text, status);
                if status == Some(30) {
                    if let Some(id) = value
                        .get("details")
                        .and_then(|d| d.get("packageId"))
                        .and_then(Value::as_str)
                    {
                        package_id = Some(id.to_string());
                    }
                    return false;
                }
                value.get("returnValue").and_then(Value::as_bool) != Some(false)
            },
        )?;
        match package_id {
            Some(id) => Ok(id),
            None => Err(NovaError::Luna(response.error_text().unwrap_or_else(
                || "установка не завершилась (не получен packageId)".into(),
            ))),
        }
    }

    /// Удаляет приложение через `appInstallService/dev/remove`.
    pub fn remove_app(&self, id: &str) -> NovaResult<()> {
        let payload = json!({ "id": id, "subscribe": true });
        let response = self.subscribe(
            "com.webos.appInstallService/dev/remove",
            &payload,
            |value| {
                let status = value.get("statusValue").and_then(Value::as_i64);
                // 20 — успешно удалено, 25 — ошибка с описанием причины.
                !matches!(status, Some(20) | Some(25))
            },
        )?;
        match response.error_text() {
            Some(error) => Err(NovaError::Luna(error)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_schemes() {
        assert_eq!(
            LunaClient::uri(LunaBus::Public, "com.webos.applicationManager/launch"),
            "luna://com.webos.applicationManager/launch"
        );
        assert_eq!(
            LunaClient::uri(
                LunaBus::Private,
                "luna://com.webos.applicationManager/launch"
            ),
            "palm://com.webos.applicationManager/launch"
        );
        assert_eq!(LunaBus::Public.binary(), "luna-send-pub");
        assert_eq!(LunaBus::Private.binary(), "luna-send");
    }

    #[test]
    fn offline_client_refuses_calls() {
        let client = LunaClient::offline();
        assert!(!client.available());
        assert!(matches!(
            client.call("com.webos.applicationManager/listApps", &json!({}), 1),
            Err(NovaError::Unsupported(_))
        ));
        assert!(client.list_apps().is_err());
    }

    #[test]
    fn response_error_extraction() {
        let response = LunaResponse {
            bus: LunaBus::Public,
            payloads: vec![
                json!({"returnValue": false, "errorCode": 404, "errorText": "no such method"}),
            ],
        };
        assert_eq!(
            response.error_text().as_deref(),
            Some("[404] no such method")
        );
        assert!(response.ok_payload().is_none());

        let ok = LunaResponse {
            bus: LunaBus::Public,
            payloads: vec![json!({"returnValue": true, "apps": []})],
        };
        assert!(ok.error_text().is_none());
        assert!(ok.ok_payload().is_some());
    }
}
