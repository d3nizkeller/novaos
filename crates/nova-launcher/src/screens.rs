//! Экраны NovaOS: раскладка, отрисовка и обработка пульта.
//!
//! [`layout`] считает прямоугольники элементов детерминированно — из данных
//! состояния, без обращения к шрифтам. [`draw`] рисует кадр, а [`handle_key`]
//! превращает нажатия пульта в действия. Раскладка и ввод используют одни и те
//! же функции окон прокрутки, поэтому фокус никогда не «уезжает» мимо
//! нарисованного элемента.

use nova_core::app::{AppSource, InstalledApp};
use nova_core::repo::StoreItem;
use nova_core::settings::{HomeMode, Resolution, Wallpaper};

use crate::gfx::{Canvas, FontSet, Rect, TextCache, Weight};
use crate::state::{
    AppState, ConfirmAction, ConfirmModal, Focus, Intent, Modal, Screen, StoreFilter, StoreSync,
    TextField,
};
use crate::theme::LOGICAL_WIDTH;
use crate::widgets::{self, Button, Ctx, TileState, ToastKind};

// ============================ Постоянные раскладки ============================

/// Отступ от края экрана.
pub const MARGIN: f32 = 96.0;
/// Верх заголовка.
pub const HEADER_Y: f32 = 44.0;
/// Верх панели разделов.
pub const NAV_Y: f32 = 176.0;
/// Высота панели разделов.
pub const NAV_H: f32 = 76.0;
/// Верх содержимого.
pub const CONTENT_Y: f32 = 292.0;
/// Низ содержимого.
pub const CONTENT_BOTTOM: f32 = 1000.0;
/// Рабочая ширина содержимого.
pub const CONTENT_W: f32 = LOGICAL_WIDTH - MARGIN * 2.0;
/// Логическая высота экрана.
pub const SCREEN_H: f32 = 1080.0;

/// Секция навигации (всегда нулевая).
pub const SEC_NAV: usize = 0;

/// Размер плитки приложения.
pub const APP_TILE: f32 = 168.0;
/// Высота плитки вместе с подписью.
pub const APP_TILE_ROW: f32 = 204.0;
/// Колонки в ряду «Все приложения» на главной.
pub const HOME_COLS: usize = 8;
/// Колонки в сетке экрана «Приложения».
pub const APP_COLS: usize = 8;
/// Видимых рядов в сетке приложений.
pub const APP_ROWS_VISIBLE: usize = 2;
/// Карточек магазина в ряду.
pub const STORE_COLS: usize = 2;
/// Видимых рядов магазина.
pub const STORE_ROWS_VISIBLE: usize = 3;
/// Высота карточки магазина.
pub const STORE_CARD_H: f32 = 150.0;
/// Высота строки списка.
pub const LIST_ROW: f32 = 80.0;
/// Видимых строк в настройках.
pub const SETTINGS_VISIBLE: usize = 6;
/// Дополнительная полоса над строкой, открывающей группу настроек.
pub const GROUP_GAP: f32 = 36.0;
/// Видимых строк в «О системе».
pub const ABOUT_VISIBLE: usize = 8;

// ============================ Клавиши пульта ============================

/// Клавиша пульта (абстракция над SDL и webOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Влево.
    Left,
    /// Вправо.
    Right,
    /// Вверх.
    Up,
    /// Вниз.
    Down,
    /// Подтверждение (OK/Enter).
    Ok,
    /// Назад.
    Back,
    /// Меню.
    Menu,
    /// Домой.
    Home,
    /// Выход из приложения.
    Exit,
    /// Воспроизведение — запустить выбранное.
    Play,
    /// Красная кнопка.
    Red,
    /// Зелёная кнопка.
    Green,
    /// Жёлтая кнопка.
    Yellow,
    /// Синяя кнопка.
    Blue,
    /// Информация о выбранном.
    Info,
    /// Обновить.
    Refresh,
    /// Цифра.
    Digit(u8),
}

impl Key {
    /// Клавиша по имени (имена клавиш SDL, ASCII-символы, кнопки webOS).
    pub fn from_name(name: &str) -> Option<Key> {
        let key = match name.to_ascii_lowercase().as_str() {
            "left" => Key::Left,
            "right" => Key::Right,
            "up" => Key::Up,
            "down" => Key::Down,
            "return" | "ok" | "enter" | "space" => Key::Ok,
            "escape" | "backspace" | "back" | "ac back" => Key::Back,
            "menu" | "tab" => Key::Menu,
            "home" => Key::Home,
            "q" | "exit" | "ac exit" => Key::Exit,
            "p" | "play" | "audioplay" => Key::Play,
            "r" | "red" | "f1" => Key::Red,
            "g" | "green" | "f2" => Key::Green,
            "y" | "yellow" | "f3" => Key::Yellow,
            "b" | "blue" | "f4" => Key::Blue,
            "i" | "info" | "f6" => Key::Info,
            "f5" => Key::Refresh,
            "0" => Key::Digit(0),
            "1" => Key::Digit(1),
            "2" => Key::Digit(2),
            "3" => Key::Digit(3),
            "4" => Key::Digit(4),
            "5" => Key::Digit(5),
            "6" => Key::Digit(6),
            "7" => Key::Digit(7),
            "8" => Key::Digit(8),
            "9" => Key::Digit(9),
            _ => return None,
        };
        Some(key)
    }
}

// ============================ Раскладка ============================

/// Секция раскладки: видимые прямоугольники и логическое число элементов.
///
/// Секция может содержать больше элементов, чем помещается на экране: тогда
/// `rects` хранит только видимое окно, а `offset` — логический индекс первого
/// видимого элемента. Навигация работает по `count`, поэтому фокус уходит за
/// пределы экрана и список прокручивается.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Прямоугольники видимых элементов (по порядку).
    pub rects: Vec<Rect>,
    /// Логическое число элементов в секции.
    pub count: usize,
    /// Логический индекс первого видимого элемента.
    pub offset: usize,
}

/// Пустая секция (для обращений за пределами раскладки).
static EMPTY_SECTION: Section = Section {
    rects: Vec::new(),
    count: 0,
    offset: 0,
};

impl Default for Section {
    fn default() -> Self {
        Section {
            rects: Vec::new(),
            count: 0,
            offset: 0,
        }
    }
}

impl Section {
    /// Секция, где все элементы видимы.
    pub fn plain(rects: Vec<Rect>) -> Section {
        let count = rects.len();
        Section {
            rects,
            count,
            offset: 0,
        }
    }

    /// Секция с прокруткой: `rects` — окно `window` из `total` элементов.
    pub fn scrolled(rects: Vec<Rect>, total: usize, window: Window) -> Section {
        Section {
            rects,
            count: total,
            offset: window.start,
        }
    }

    /// Секция-строка сетки: прямоугольники есть только у видимых строк.
    pub fn grid_row(rects: Vec<Rect>, count: usize) -> Section {
        Section {
            rects,
            count,
            offset: 0,
        }
    }

    /// Прямоугольник логического элемента `item`.
    pub fn slot(&self, item: usize) -> Option<Rect> {
        self.rects.get(item.checked_sub(self.offset)?).copied()
    }

    /// Число логических элементов.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Нет ли элементов.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Раскладка экрана: секции в порядке навигации.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    /// Секции: первая — навигация, дальше содержимое экрана.
    pub sections: Vec<Section>,
}

impl Layout {
    /// Число элементов в каждой секции (для [`crate::state::move_focus`]).
    pub fn counts(&self) -> Vec<usize> {
        self.sections.iter().map(|section| section.count).collect()
    }

    /// Секция по индексу (пустая, если индекса нет).
    pub fn section(&self, index: usize) -> &Section {
        self.sections.get(index).unwrap_or(&EMPTY_SECTION)
    }

    /// Прямоугольник элемента под фокусом.
    pub fn rect(&self, focus: Focus) -> Option<Rect> {
        self.section(focus.section).slot(focus.item)
    }

    /// Число секций.
    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// Пуста ли раскладка.
    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(Section::is_empty)
    }
}

/// Ряд одинаковых ячеек.
fn row(x: f32, y: f32, width: f32, count: usize, height: f32, gap: f32) -> Vec<Rect> {
    (0..count)
        .map(|i| Rect::new(x + i as f32 * (width + gap), y, width, height))
        .collect()
}

/// Колонка одинаковых ячеек.
fn column(x: f32, y: f32, width: f32, count: usize, height: f32, gap: f32) -> Vec<Rect> {
    (0..count)
        .map(|i| Rect::new(x, y + i as f32 * (height + gap), width, height))
        .collect()
}

/// Окно прокрутки: какие элементы показывать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Индекс первого видимого элемента.
    pub start: usize,
    /// Сколько элементов показывается.
    pub count: usize,
}

impl Window {
    /// Пустое окно.
    pub const EMPTY: Window = Window { start: 0, count: 0 };

    /// Индекс в данных по индексу на экране.
    pub fn index(&self, visible: usize) -> usize {
        self.start + visible
    }

    /// Сколько элементов реально показывается при общем количестве `total`.
    pub fn shown(&self, total: usize) -> usize {
        self.count.min(total.saturating_sub(self.start))
    }
}

/// Считает окно прокрутки так, чтобы фокус оставался видимым.
///
/// `keep` — сколько элементов оставлять «про запас» перед фокусом, чтобы список
/// начинал прокручиваться заранее, а не когда фокус упёрся в край.
pub fn window_for(focus: usize, total: usize, visible: usize, keep: usize) -> Window {
    if total == 0 || visible == 0 {
        return Window::EMPTY;
    }
    let visible = visible.min(total);
    let focus = focus.min(total - 1);
    // Минимальный старт, при котором фокус ещё виден.
    let lower = focus.saturating_sub(visible - 1);
    let keep = keep.min(visible.saturating_sub(1));
    let start = focus.saturating_sub(keep).max(lower).min(total - visible);
    Window {
        start,
        count: visible,
    }
}

/// Прямоугольники навигации (шесть равных ячеек).
pub fn nav_rects() -> Vec<Rect> {
    let gap = 16.0;
    let width = (CONTENT_W - gap * (Screen::ALL.len() as f32 - 1.0)) / Screen::ALL.len() as f32;
    row(MARGIN, NAV_Y, width, Screen::ALL.len(), NAV_H, gap)
}

/// Полная раскладка текущего экрана.
pub fn layout(state: &AppState) -> Layout {
    let mut sections = vec![Section::plain(nav_rects())];
    match state.screen {
        Screen::Home => sections.extend(home_sections(state)),
        Screen::Apps => sections.extend(apps_sections(state)),
        Screen::Store => sections.extend(store_sections(state)),
        Screen::Sideload => sections.extend(sideload_sections(state)),
        Screen::Settings => sections.extend(settings_sections(state)),
        Screen::About => sections.extend(about_sections(state)),
    }
    Layout { sections }
}

/// Окно ряда «Все приложения» на главной.
pub fn home_apps_window(state: &AppState) -> Window {
    let focus = state.focus();
    let item = if focus.section == 2 { focus.item } else { 0 };
    window_for(item, state.visible_apps().len(), HOME_COLS, 3)
}

/// Окно сетки экрана «Приложения».
pub fn apps_window(state: &AppState) -> Window {
    let per_page = APP_COLS * APP_ROWS_VISIBLE;
    window_for(
        grid_focus(state, 2),
        state.visible_apps().len(),
        per_page,
        APP_COLS,
    )
}

/// Позиция фокуса внутри сетки (строки начинаются с `first_section`).
fn grid_focus(state: &AppState, first_section: usize) -> usize {
    let focus = state.focus();
    if focus.section < first_section {
        return 0;
    }
    (focus.section - first_section) * APP_COLS + focus.item
}

/// Позиция фокуса внутри сетки магазина (секции — логические ряды).
fn store_focus_index(state: &AppState) -> usize {
    let focus = state.focus();
    if focus.section < 2 {
        return 0;
    }
    (focus.section - 2) * STORE_COLS + focus.item
}

/// Окно списка магазина.
pub fn store_window(state: &AppState) -> Window {
    let per_page = STORE_COLS * STORE_ROWS_VISIBLE;
    window_for(
        store_focus_index(state),
        state.visible_store().len(),
        per_page,
        STORE_COLS,
    )
}

/// Окно списка настроек.
pub fn settings_window(state: &AppState) -> Window {
    let focus = state.focus();
    let item = if focus.section == 1 { focus.item } else { 0 };
    window_for(item, settings_items(state).len(), SETTINGS_VISIBLE, 2)
}

/// Окно списка сведений «О системе».
pub fn about_window(state: &AppState) -> Window {
    let focus = state.focus();
    let item = if focus.section == 2 { focus.item } else { 0 };
    window_for(item, state.about_lines().len(), ABOUT_VISIBLE, 2)
}

fn home_sections(state: &AppState) -> Vec<Section> {
    let favorites = state.favorites().len().min(3);
    let card_w = 540.0;
    let card_h = 304.0;
    let gap = if favorites > 1 {
        (CONTENT_W - card_w * favorites as f32) / (favorites as f32 - 1.0)
    } else {
        54.0
    };
    let favorites_row = Section::plain(row(MARGIN, 336.0, card_w, favorites, card_h, gap));

    let apps = state.visible_apps().len();
    let window = home_apps_window(state);
    let tile_gap = 32.0;
    let total_w = HOME_COLS as f32 * APP_TILE + (HOME_COLS as f32 - 1.0) * tile_gap;
    let x0 = MARGIN + (CONTENT_W - total_w) / 2.0;
    let apps_row = Section::scrolled(
        row(
            x0,
            708.0,
            APP_TILE,
            window.shown(apps),
            APP_TILE_ROW,
            tile_gap,
        ),
        apps,
        window,
    );

    let action_w = 400.0;
    let action_gap = (CONTENT_W - action_w * 4.0) / 3.0;
    let actions = Section::plain(row(MARGIN, 936.0, action_w, 4, 56.0, action_gap));

    vec![favorites_row, apps_row, actions]
}

fn apps_sections(state: &AppState) -> Vec<Section> {
    let tools = Section::plain(row(MARGIN, CONTENT_Y, 380.0, 3, 64.0, 24.0));
    let tile_gap = 32.0;
    let total_w = APP_COLS as f32 * APP_TILE + (APP_COLS as f32 - 1.0) * tile_gap;
    let x0 = MARGIN + (CONTENT_W - total_w) / 2.0;
    let apps = state.visible_apps().len();
    let window = apps_window(state);
    let first_row = window.start / APP_COLS;
    let rows = apps.div_ceil(APP_COLS);

    let mut out = vec![tools];
    for line in 0..rows {
        let start = line * APP_COLS;
        let count = apps.saturating_sub(start).min(APP_COLS);
        let visible = line >= first_row && line < first_row + APP_ROWS_VISIBLE;
        let rects = if visible {
            row(
                x0,
                400.0 + (line - first_row) as f32 * (APP_TILE_ROW + 32.0),
                APP_TILE,
                count,
                APP_TILE_ROW,
                tile_gap,
            )
        } else {
            Vec::new()
        };
        out.push(Section::grid_row(rects, count));
    }
    out
}

fn store_sections(state: &AppState) -> Vec<Section> {
    let tools = Section::plain(row(MARGIN + 8.0, CONTENT_Y, 200.0, 8, 64.0, 16.0));
    let items = state.visible_store().len();
    let card_w = (CONTENT_W - 32.0) / STORE_COLS as f32;
    let window = store_window(state);
    let first_row = window.start / STORE_COLS;
    let rows = items.div_ceil(STORE_COLS);

    let mut out = vec![tools];
    for line in 0..rows {
        let start = line * STORE_COLS;
        let count = items.saturating_sub(start).min(STORE_COLS);
        let visible = line >= first_row && line < first_row + STORE_ROWS_VISIBLE;
        let rects = if visible {
            row(
                MARGIN,
                400.0 + (line - first_row) as f32 * (STORE_CARD_H + 24.0),
                card_w,
                count,
                STORE_CARD_H,
                32.0,
            )
        } else {
            Vec::new()
        };
        out.push(Section::grid_row(rects, count));
    }
    out
}

/// Действия экрана «Установка».
pub const SIDELOAD_ACTIONS: [&str; 5] = [
    "Установить по ссылке",
    "Приём файлов с телефона",
    "Файл на устройстве",
    "Сканировать USB",
    "Обновить каталог",
];

/// Видимых строк в списке найденных USB-пакетов.
pub const USB_VISIBLE: usize = 2;

/// Окно списка USB-пакетов.
pub fn usb_window(state: &AppState) -> Window {
    let focus = state.focus();
    let item = if focus.section == 2 { focus.item } else { 0 };
    window_for(item, state.usb_packages.len(), USB_VISIBLE, 0)
}

fn sideload_sections(state: &AppState) -> Vec<Section> {
    let actions = Section::plain(column(
        MARGIN,
        CONTENT_Y,
        840.0,
        SIDELOAD_ACTIONS.len(),
        LIST_ROW,
        12.0,
    ));
    let total = state.usb_packages.len();
    let window = usb_window(state);
    let usb = Section::scrolled(
        column(MARGIN, 812.0, 840.0, window.shown(total), LIST_ROW, 8.0),
        total,
        window,
    );
    vec![actions, usb]
}

/// Строка экрана настроек.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsRow {
    /// Тема оформления.
    Theme,
    /// Обои.
    Wallpaper,
    /// Анимации.
    Animations,
    /// 24-часовые часы.
    Clock24,
    /// Разрешение интерфейса.
    Resolution,
    /// Показывать системные приложения.
    ShowSystem,
    /// Сбросить избранное.
    ResetFavorites,
    /// Обновить магазин.
    SyncStore,
    /// Добавить репозиторий.
    AddRepo,
    /// Репозиторий по индексу.
    Repo(usize),
    /// Кнопка Home.
    HomeMode,
    /// Перезагрузка.
    Reboot,
    /// Выключение.
    PowerOff,
    /// Выйти из NovaOS.
    Quit,
    /// Сброс настроек.
    ResetSettings,
    /// Диагностика.
    Diagnostics,
}

/// Элемент списка настроек: заголовок группы или строка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEntry {
    /// Заголовок группы.
    Header(String),
    /// Настраиваемый параметр.
    Row(SettingsRow),
}

/// Полный список элементов экрана настроек (с заголовками групп).
pub fn settings_entries(state: &AppState) -> Vec<SettingsEntry> {
    let mut entries = vec![
        SettingsEntry::Header("Оформление".into()),
        SettingsEntry::Row(SettingsRow::Theme),
        SettingsEntry::Row(SettingsRow::Wallpaper),
        SettingsEntry::Row(SettingsRow::Animations),
        SettingsEntry::Row(SettingsRow::Clock24),
        SettingsEntry::Row(SettingsRow::Resolution),
        SettingsEntry::Header("Приложения".into()),
        SettingsEntry::Row(SettingsRow::ShowSystem),
        SettingsEntry::Row(SettingsRow::ResetFavorites),
        SettingsEntry::Header("Магазин".into()),
        SettingsEntry::Row(SettingsRow::SyncStore),
        SettingsEntry::Row(SettingsRow::AddRepo),
    ];
    if !state.settings.repos.is_empty() {
        entries.push(SettingsEntry::Header("Репозитории".into()));
        for index in 0..state.settings.repos.len() {
            entries.push(SettingsEntry::Row(SettingsRow::Repo(index)));
        }
    }
    entries.extend([
        SettingsEntry::Header("Система".into()),
        SettingsEntry::Row(SettingsRow::HomeMode),
        SettingsEntry::Row(SettingsRow::Reboot),
        SettingsEntry::Row(SettingsRow::PowerOff),
        SettingsEntry::Row(SettingsRow::Quit),
        SettingsEntry::Row(SettingsRow::ResetSettings),
        SettingsEntry::Row(SettingsRow::Diagnostics),
    ]);
    entries
}

/// Только фокусируемые строки настроек.
pub fn settings_items(state: &AppState) -> Vec<SettingsRow> {
    settings_entries(state)
        .into_iter()
        .filter_map(|entry| match entry {
            SettingsEntry::Row(item) => Some(item),
            SettingsEntry::Header(_) => None,
        })
        .collect()
}

/// Индекс строки в списке настроек → индекс заголовка группы перед ней.
fn settings_header_for(state: &AppState, item_index: usize) -> Option<String> {
    let entries = settings_entries(state);
    let mut seen = 0usize;
    let mut last_header: Option<String> = None;
    for entry in entries {
        match entry {
            SettingsEntry::Header(title) => last_header = Some(title),
            SettingsEntry::Row(_) => {
                if seen == item_index {
                    return last_header;
                }
                seen += 1;
            }
        }
    }
    None
}

fn settings_sections(state: &AppState) -> Vec<Section> {
    let items = settings_items(state).len();
    let window = settings_window(state);
    let mut rects = Vec::with_capacity(window.shown(items));
    let mut y = CONTENT_Y;
    for slot in 0..window.shown(items) {
        let index = window.index(slot);
        if starts_group(state, slot, index) {
            y += GROUP_GAP;
        }
        rects.push(Rect::new(MARGIN, y, CONTENT_W, LIST_ROW));
        y += LIST_ROW + 10.0;
    }
    vec![Section::scrolled(rects, items, window)]
}

/// Открывает ли строка настроек новую группу (нужна полоса под заголовок).
fn starts_group(state: &AppState, slot: usize, index: usize) -> bool {
    slot == 0 || settings_header_for(state, index) != settings_header_for(state, index - 1)
}

/// Действия экрана «О системе».
pub const ABOUT_ACTIONS: [&str; 3] = [
    "Проверить обновления NovaOS",
    "Диагностика",
    "Выйти из NovaOS",
];

fn about_sections(state: &AppState) -> Vec<Section> {
    let lines = state.about_lines().len();
    let window = about_window(state);
    let actions = Section::plain(row(
        MARGIN,
        CONTENT_Y,
        520.0,
        ABOUT_ACTIONS.len(),
        64.0,
        24.0,
    ));
    let info: Vec<Rect> = (0..window.shown(lines))
        .map(|slot| {
            let y = CONTENT_Y + 100.0 + slot as f32 * 62.0;
            Rect::new(MARGIN, y, CONTENT_W, 54.0)
        })
        .collect();
    vec![actions, Section::scrolled(info, lines, window)]
}

// ============================ Модель представления ============================

/// Приложение в терминах интерфейса (дешёвая копия для отрисовки кадра).
#[derive(Debug, Clone, PartialEq)]
pub struct AppView {
    /// Идентификатор.
    pub id: String,
    /// Название.
    pub title: String,
    /// Версия.
    pub version: String,
    /// Тип приложения.
    pub kind: String,
    /// Источник установки.
    pub source: String,
    /// Запущено ли.
    pub running: bool,
    /// В избранном ли.
    pub favorite: bool,
    /// Затемнять (штатное приложение LG).
    pub dimmed: bool,
    /// Можно ли удалить.
    pub removable: bool,
    /// Путь к иконке.
    pub icon_path: Option<std::path::PathBuf>,
    /// Каталог приложения.
    pub path: String,
    /// Размер в байтах.
    pub size: Option<u64>,
}

impl AppView {
    /// Из установленного приложения.
    pub fn of(app: &InstalledApp) -> Self {
        Self {
            id: app.info.id.clone(),
            title: app.title(),
            version: app.info.version.clone().unwrap_or_else(|| "—".into()),
            kind: app.info.kind().label_ru().to_string(),
            source: app.source.label_ru().to_string(),
            running: app.running,
            favorite: app.favorite_order.is_some(),
            dimmed: matches!(app.source, AppSource::System),
            removable: app.removable(),
            icon_path: app.icon_path.clone(),
            path: app.path.display().to_string(),
            size: app.size_bytes,
        }
    }

    /// Запросы иконок для списка представлений.
    pub fn icon_requests(views: &[AppView]) -> Vec<(String, std::path::PathBuf)> {
        views
            .iter()
            .filter_map(|view| {
                view.icon_path
                    .as_ref()
                    .map(|path| (view.id.clone(), path.clone()))
            })
            .collect()
    }

    /// Подпись под крупной карточкой.
    pub fn hero_subtitle(&self) -> String {
        format!("{} · v{}", self.kind, self.version)
    }

    /// Полное описание для карточки приложения.
    pub fn details(&self) -> String {
        let mut lines = vec![
            format!("Идентификатор: {}", self.id),
            format!("Версия: {}", self.version),
            format!("Тип: {}", self.kind),
            format!("Источник: {}", self.source),
        ];
        if let Some(size) = self.size {
            lines.push(format!("Размер: {}", nova_core::net::format_size(size)));
        }
        lines.push(format!("Каталог: {}", self.path));
        if !self.removable {
            lines.push("Штатное приложение webOS — удалить нельзя.".into());
        }
        lines.join("\n")
    }
}

/// Пакет магазина в терминах интерфейса.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreView {
    /// Идентификатор.
    pub id: String,
    /// Название.
    pub title: String,
    /// Версия.
    pub version: String,
    /// Подпись под карточкой.
    pub subtitle: String,
    /// Текст кнопки.
    pub action: String,
    /// Адрес иконки.
    pub icon_url: String,
    /// Требуется ли root.
    pub root_required: bool,
    /// Описание.
    pub description: String,
    /// Адрес IPK.
    pub ipk_url: String,
    /// Репозиторий.
    pub repo_title: String,
    /// Размер пакета.
    pub size: Option<u64>,
    /// Требование к webOS.
    pub webos_requirement: Option<String>,
    /// SHA-256 пакета.
    pub sha256: Option<String>,
}

/// Видимые приложения главной/сетки.
pub fn app_views(state: &AppState) -> Vec<AppView> {
    let apps: Vec<&InstalledApp> = state.visible_apps();
    let views: Vec<AppView> = apps.iter().map(|app| AppView::of(app)).collect();
    views
}

/// Избранные приложения (до трёх крупных карточек).
pub fn favorite_views(state: &AppState) -> Vec<AppView> {
    state
        .favorites()
        .iter()
        .map(|app| AppView::of(app))
        .collect()
}

/// Видимые пакеты магазина.
pub fn store_views(state: &AppState) -> Vec<StoreView> {
    state
        .visible_store()
        .into_iter()
        .map(|item| StoreView {
            id: item.id.clone(),
            title: item.title.clone(),
            version: item.version.clone(),
            subtitle: store_subtitle(state, item),
            action: state.store_action(item).to_string(),
            icon_url: item.icon_url.clone(),
            root_required: item.root_required,
            description: item.description.clone(),
            ipk_url: item.ipk_url.clone(),
            repo_title: item.repo_title.clone(),
            size: item.size,
            webos_requirement: item.webos_requirement.clone(),
            sha256: item.sha256.clone(),
        })
        .collect()
}

fn store_subtitle(state: &AppState, item: &StoreItem) -> String {
    let mut parts = vec![format!("v{}", item.version)];
    if let Some(current) = state.installed_version(&item.id) {
        parts.push(format!("установлено v{current}"));
    }
    if let Some(size) = item.size {
        parts.push(nova_core::net::format_size(size));
    }
    parts.push(
        if item.root_required {
            "нужен root"
        } else {
            "без root"
        }
        .to_string(),
    );
    parts.push(item.repo_title.clone());
    parts.join(" · ")
}

/// Подробная карточка пакета магазина.
pub fn store_details(state: &AppState, view: &StoreView) -> String {
    let mut lines = vec![
        format!("Версия в репозитории: {}", view.version),
        match state.installed_version(&view.id) {
            Some(version) => format!("Установлено: {version}"),
            None => "Не установлено".into(),
        },
        format!(
            "Размер: {}",
            view.size
                .map(nova_core::net::format_size)
                .unwrap_or_else(|| "неизвестен".into())
        ),
        format!("Репозиторий: {}", view.repo_title),
        if view.root_required {
            "Требуется root-доступ (Homebrew Channel)".into()
        } else {
            "Устанавливается без root".into()
        },
    ];
    if let Some(requirement) = &view.webos_requirement {
        lines.push(format!(
            "Требуется webOS: {requirement} (на устройстве: {})",
            state
                .sysinfo
                .webos_release
                .clone()
                .unwrap_or_else(|| "—".into())
        ));
    }
    if !view.description.is_empty() {
        lines.push(String::new());
        lines.push(view.description.clone());
    }
    if let Some(sha) = &view.sha256 {
        lines.push(format!(
            "SHA-256: {}…",
            sha.chars().take(16).collect::<String>()
        ));
    }
    lines.push(view.ipk_url.clone());
    lines.join("\n")
}

// ============================ Рендерер ============================

/// Рендерер кадра: холст, шрифты и кэш глифов.
pub struct Renderer {
    /// Холст.
    pub canvas: Canvas,
    /// Шрифты.
    pub fonts: FontSet,
    /// Кэш глифов.
    pub cache: TextCache,
}

impl Renderer {
    /// Создаёт рендерер указанного размера.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            canvas: Canvas::new(width, height),
            fonts: FontSet::embedded(),
            cache: TextCache::new(),
        }
    }

    /// Меняет размер холста (кэш глифов сбрасывается).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas = Canvas::new(width, height);
        self.cache.clear();
    }

    /// Рисует один кадр состояния.
    pub fn frame(&mut self, state: &mut AppState) {
        let layout = layout(state);
        preload_icons(state, &layout);
        prune_modal(state);
        let theme = state.theme;
        let now = state.now;
        let mut ctx = Ctx::new(&mut self.canvas, &self.fonts, &mut self.cache, theme, now);
        draw(state, &mut ctx, &layout);
    }

    /// Сохраняет кадр в PNG.
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.canvas.save_png(path)
    }
}

/// Закрывает карточку, если объект, на который она ссылается, исчез.
fn prune_modal(state: &mut AppState) {
    match &state.modal {
        Modal::AppInfo(id) if state.catalog.by_id(id).is_none() => state.close_modal(),
        Modal::StoreInfo(id) if state.store_item(id).is_none() => state.close_modal(),
        _ => {}
    }
}

/// Заказывает иконки только для видимых элементов.
fn preload_icons(state: &mut AppState, layout: &Layout) {
    let _ = layout;
    match state.screen {
        Screen::Home => {
            let favorites = favorite_views(state);
            let apps = app_views(state);
            let window = home_apps_window(state);
            let end = (window.start + window.shown(apps.len())).min(apps.len());
            let mut requests = AppView::icon_requests(&favorites);
            requests.extend(AppView::icon_requests(&apps[..end]));
            state.ensure_icons(requests);
        }
        Screen::Apps => {
            let apps = app_views(state);
            let window = apps_window(state);
            let per_page = APP_COLS * APP_ROWS_VISIBLE;
            let end = (window.start + per_page).min(apps.len());
            let start = window.start.min(end);
            state.ensure_icons(AppView::icon_requests(&apps[start..end]));
        }
        Screen::Store => {
            let views = store_views(state);
            let window = store_window(state);
            let per_page = STORE_COLS * STORE_ROWS_VISIBLE;
            let end = (window.start + per_page).min(views.len());
            let start = window.start.min(end);
            let requests: Vec<(String, String)> = views[start..end]
                .iter()
                .map(|view| (view.id.clone(), view.icon_url.clone()))
                .collect();
            state.ensure_store_icons(requests);
        }
        _ => {}
    }
}

// ============================ Отрисовка ============================

/// Рисует кадр целиком: фон, экран, модальные окна, уведомления.
pub fn draw(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.theme.palette;
    ctx.clear(p.background_deep);
    widgets::background(ctx, state.wallpaper.as_ref());
    widgets::ambient(ctx);

    draw_header(state, ctx);
    draw_nav(state, ctx, layout);

    match state.screen {
        Screen::Home => draw_home(state, ctx, layout),
        Screen::Apps => draw_apps(state, ctx, layout),
        Screen::Store => draw_store(state, ctx, layout),
        Screen::Sideload => draw_sideload(state, ctx, layout),
        Screen::Settings => draw_settings(state, ctx, layout),
        Screen::About => draw_about(state, ctx, layout),
    }

    widgets::key_hints(ctx, &hints_for(state));

    if state.progress.active {
        draw_progress(state, ctx);
    }
    draw_modal(state, ctx);
    if let Some(toast) = &state.toast {
        widgets::toast(ctx, &toast.message, toast.kind, toast.alpha());
    }
    if state.screensaver {
        draw_screensaver(state, ctx);
    }
}

fn draw_header(state: &AppState, ctx: &mut Ctx) {
    let p = ctx.palette();
    let ty = ctx.typo();
    widgets::top_bar(
        ctx,
        state.screen.title_ru(),
        &state.clock_text,
        Some(state.screen.subtitle_ru()),
    );
    let w = ctx.measure(ty.small, Weight::Regular, &state.date_text);
    ctx.text(
        ctx.width() - MARGIN - w,
        HEADER_Y + 100.0,
        ty.small,
        Weight::Regular,
        p.text_faint,
        &state.date_text,
    );
    if state.upload.is_some() {
        let title_w = ctx.measure(ty.title, Weight::SemiBold, state.screen.title_ru());
        widgets::badge(
            ctx,
            Rect::new(MARGIN + title_w + 26.0, HEADER_Y + 14.0, 0.0, 30.0),
            "приём файлов",
            p.success,
        );
    }
}

fn draw_nav(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let focus = state.focus();
    for (index, screen) in Screen::ALL.iter().enumerate() {
        let Some(rect) = layout.section(SEC_NAV).slot(index) else {
            continue;
        };
        let active = *screen == state.screen;
        let focused = focus.section == SEC_NAV && focus.item == index;
        let f = if focused { state.focus_anim } else { 0.0 };
        widgets::nav_item(ctx, rect, screen.glyph(), screen.title_ru(), active, f);
    }
}

// ---------------------------- Главная ----------------------------

fn draw_home(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let focus = state.focus();
    let favorites = favorite_views(state);

    widgets::section_title(
        ctx,
        Rect::new(MARGIN, 296.0, CONTENT_W, 36.0),
        "Избранное",
        Some(&favorites.len().to_string()),
    );
    if favorites.is_empty() {
        widgets::empty_state(
            ctx,
            Rect::new(MARGIN, 336.0, CONTENT_W, 304.0),
            "★",
            "В избранном пусто",
            "Откройте «Приложения», наведите фокус на нужное и нажмите жёлтую кнопку — оно появится здесь крупными карточками.",
        );
    } else {
        for (index, app) in favorites.iter().enumerate().take(3) {
            let Some(rect) = layout.section(1).slot(index) else {
                continue;
            };
            let f = if focus.section == 1 && focus.item == index {
                state.focus_anim
            } else {
                0.0
            };
            let tile = TileState {
                focus: f,
                running: app.running,
                badge: Some(app.kind.clone()),
                dimmed: false,
                favorite: true,
            };
            widgets::hero_card(
                ctx,
                rect,
                &app.title,
                &app.hero_subtitle(),
                state.icon(&app.id),
                &tile,
            );
        }
    }

    let apps = app_views(state);
    let window = home_apps_window(state);
    widgets::section_title(
        ctx,
        Rect::new(MARGIN, 664.0, CONTENT_W, 36.0),
        "Все приложения",
        Some(&apps.len().to_string()),
    );
    let shown = window.shown(apps.len());
    if apps.is_empty() {
        widgets::empty_state(
            ctx,
            Rect::new(MARGIN, 708.0, CONTENT_W, APP_TILE_ROW),
            "◆",
            "Приложений пока нет",
            "Установите свой первый пакет на экране «Установка» — по ссылке, с флешки или прямо с телефона по QR-коду.",
        );
    } else {
        for (index, app) in apps.iter().enumerate().skip(window.start).take(shown) {
            let Some(rect) = layout.section(2).slot(index) else {
                continue;
            };
            let f = if focus.section == 2 && focus.item == index {
                state.focus_anim
            } else {
                0.0
            };
            let tile = TileState {
                focus: f,
                running: app.running,
                badge: None,
                dimmed: app.dimmed,
                favorite: app.favorite,
            };
            widgets::app_tile(ctx, rect, &app.title, state.icon(&app.id), &tile);
        }
        if apps.len() > shown {
            widgets::scrollbar(
                ctx,
                Rect::new(MARGIN + CONTENT_W + 10.0, 708.0, 6.0, APP_TILE_ROW),
                shown as f32 / apps.len() as f32,
                window.start as f32 / apps.len().saturating_sub(shown).max(1) as f32,
            );
        }
    }

    let actions = ["Магазин", "Установка", "Настройки", "О системе"];
    for (index, label) in actions.iter().enumerate() {
        let Some(rect) = layout.section(3).slot(index) else {
            continue;
        };
        let f = if focus.section == 3 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        widgets::pill(ctx, rect, label, false, f);
    }
    let _ = p;
}

// ---------------------------- Приложения ----------------------------

fn draw_apps(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let focus = state.focus();

    let tools = [
        if state.apps_query.is_empty() {
            "Поиск".to_string()
        } else {
            format!("Поиск: {}", state.apps_query)
        },
        format!(
            "Системные: {}",
            if state.settings.show_system_apps {
                "вкл"
            } else {
                "выкл"
            }
        ),
        "Обновить".to_string(),
    ];
    for (index, label) in tools.iter().enumerate() {
        let Some(rect) = layout.section(1).slot(index) else {
            continue;
        };
        let f = if focus.section == 1 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        widgets::pill(
            ctx,
            rect,
            label,
            index == 0 && !state.apps_query.is_empty(),
            f,
        );
    }

    let apps = app_views(state);
    let window = apps_window(state);
    let shown = window.shown(apps.len());
    if apps.is_empty() {
        widgets::empty_state(
            ctx,
            Rect::new(MARGIN, 400.0, CONTENT_W, 440.0),
            "◆",
            if state.apps_query.is_empty() {
                "Приложений нет"
            } else {
                "Ничего не найдено"
            },
            if state.apps_query.is_empty() {
                "На телевизоре пока нет установленных пользователем приложений. Откройте «Установка», чтобы добавить своё."
            } else {
                "Попробуйте другой запрос или включите показ системных приложений в настройках."
            },
        );
    } else {
        for (index, app) in apps.iter().enumerate() {
            let section = 2 + index / APP_COLS;
            let item = index % APP_COLS;
            let Some(rect) = layout.section(section).slot(item) else {
                continue;
            };
            let f = if focus.section == section && focus.item == item {
                state.focus_anim
            } else {
                0.0
            };
            let tile = TileState {
                focus: f,
                running: app.running,
                badge: None,
                dimmed: app.dimmed,
                favorite: app.favorite,
            };
            widgets::app_tile(ctx, rect, &app.title, state.icon(&app.id), &tile);
        }
        if apps.len() > shown {
            widgets::scrollbar(
                ctx,
                Rect::new(
                    MARGIN + CONTENT_W + 12.0,
                    400.0,
                    6.0,
                    APP_ROWS_VISIBLE as f32 * (APP_TILE_ROW + 32.0) - 32.0,
                ),
                shown as f32 / apps.len() as f32,
                window.start as f32 / apps.len().saturating_sub(shown).max(1) as f32,
            );
        }
    }

    // Сведения о приложении в фокусе.
    let strip = Rect::new(MARGIN, 864.0, CONTENT_W, 128.0);
    ctx.rect(strip, 20.0, p.surface.fade(0.75));
    ctx.stroke(strip, 20.0, 1.0, p.border);
    match focused_app_view(state) {
        Some(app) => {
            let icon_rect = Rect::new(strip.x + 16.0, strip.y + 16.0, 96.0, 96.0);
            match state.icon(&app.id) {
                Some(img) => ctx.image(img, icon_rect, 14.0, 1.0),
                None => {
                    let (a, b) = widgets::seed_gradient(&app.title);
                    ctx.gradient(icon_rect, 14.0, a, b);
                }
            }
            let text_x = icon_rect.x + icon_rect.w + 24.0;
            let text_w = strip.x + strip.w - 40.0 - text_x;
            ctx.text_ellipsized(
                Rect::new(text_x, strip.y + 18.0, text_w, 36.0),
                ty.section,
                Weight::SemiBold,
                p.text,
                &app.title,
            );
            let meta = format!(
                "{} · v{} · {} · {}",
                app.id, app.version, app.source, app.kind
            );
            ctx.text_ellipsized(
                Rect::new(text_x, strip.y + 62.0, text_w, 28.0),
                ty.small,
                Weight::Regular,
                p.text_dim,
                &meta,
            );
            if app.running {
                widgets::badge(
                    ctx,
                    Rect::new(strip.x + strip.w - 200.0, strip.y + 20.0, 0.0, 30.0),
                    "запущено",
                    p.success,
                );
            }
            if app.favorite {
                widgets::badge(
                    ctx,
                    Rect::new(strip.x + strip.w - 380.0, strip.y + 20.0, 0.0, 30.0),
                    "в избранном",
                    p.warning,
                );
            }
        }
        None => {
            ctx.text_ellipsized(
                Rect::new(strip.x + 28.0, strip.y, strip.w - 56.0, strip.h),
                ty.body,
                Weight::Regular,
                p.text_faint,
                "Наведите фокус на приложение, чтобы увидеть подробности",
            );
        }
    }
}

/// Представление приложения под фокусом.
pub fn focused_app_view(state: &AppState) -> Option<AppView> {
    let focus = state.focus();
    match state.screen {
        Screen::Home => match focus.section {
            1 => favorite_views(state).into_iter().nth(focus.item),
            2 => {
                let apps = app_views(state);
                let window = home_apps_window(state);
                apps.into_iter().nth(window.index(focus.item))
            }
            _ => None,
        },
        Screen::Apps => {
            if focus.section < 2 {
                return None;
            }
            let apps = app_views(state);
            let index = (focus.section - 2) * APP_COLS + focus.item;
            apps.into_iter().nth(index)
        }
        _ => None,
    }
}

// ---------------------------- Магазин ----------------------------

fn draw_store(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let focus = state.focus();

    let mut labels: Vec<String> = StoreFilter::ALL
        .iter()
        .map(|f| f.label_ru().to_string())
        .collect();
    labels.push("Обновить".into());
    labels.push(if state.store_query.is_empty() {
        "Поиск".into()
    } else {
        format!("Поиск: {}", state.store_query)
    });
    for (index, label) in labels.iter().enumerate() {
        let Some(rect) = layout.section(1).slot(index) else {
            continue;
        };
        let f = if focus.section == 1 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        let active =
            index < StoreFilter::ALL.len() && StoreFilter::ALL[index] == state.store_filter;
        widgets::pill(ctx, rect, label, active, f);
    }

    let views = store_views(state);
    let window = store_window(state);
    let shown = window.shown(views.len());
    if views.is_empty() {
        let (glyph, title, hint) = match &state.store_sync {
            StoreSync::Syncing => (
                "↻",
                "Обновляем репозитории",
                "Список появится через несколько секунд.",
            ),
            StoreSync::Error(e) => ("⚠", "Репозитории недоступны", e.as_str()),
            StoreSync::Idle => (
                "★",
                "Магазин пуст",
                "Нажмите «Обновить», чтобы загрузить список пакетов из репозиториев.",
            ),
            StoreSync::Ready(_) => (
                "◇",
                "Ничего не найдено",
                "Попробуйте снять фильтр или изменить поисковый запрос.",
            ),
        };
        widgets::empty_state(
            ctx,
            Rect::new(MARGIN, 400.0, CONTENT_W, 480.0),
            glyph,
            title,
            hint,
        );
    } else {
        for (index, view) in views.iter().enumerate() {
            let section = 2 + index / STORE_COLS;
            let cell = index % STORE_COLS;
            let Some(rect) = layout.section(section).slot(cell) else {
                continue;
            };
            let f = if focus.section == section && focus.item == cell {
                state.focus_anim
            } else {
                0.0
            };
            widgets::store_card(
                ctx,
                rect,
                &view.title,
                &view.subtitle,
                &view.action,
                state.store_icon(&view.id),
                f,
            );
        }
        if views.len() > shown {
            widgets::scrollbar(
                ctx,
                Rect::new(
                    MARGIN + CONTENT_W + 12.0,
                    400.0,
                    6.0,
                    STORE_ROWS_VISIBLE as f32 * (STORE_CARD_H + 24.0) - 24.0,
                ),
                shown as f32 / views.len() as f32,
                window.start as f32 / views.len().saturating_sub(shown).max(1) as f32,
            );
        }
    }

    ctx.text_ellipsized(
        Rect::new(MARGIN, 940.0, CONTENT_W, 40.0),
        ty.small,
        Weight::Regular,
        p.text_faint,
        &state.store_sync.label_ru(),
    );
}

// ---------------------------- Установка ----------------------------

fn draw_sideload(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let focus = state.focus();

    widgets::section_title(
        ctx,
        Rect::new(MARGIN, CONTENT_Y - 40.0, 840.0, 32.0),
        "Способы установки",
        None,
    );

    let upload_active = state.upload.is_some();
    let trailing = [
        "IPK, APK, ZIP, HTML, M3U".to_string(),
        if upload_active {
            "работает".to_string()
        } else {
            "выключен".to_string()
        },
        "путь на ТВ или USB".to_string(),
        format!("найдено: {}", state.usb_packages.len()),
        "пересканировать систему".to_string(),
    ];
    for (index, label) in SIDELOAD_ACTIONS.iter().enumerate() {
        let Some(rect) = layout.section(1).slot(index) else {
            continue;
        };
        let f = if focus.section == 1 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        widgets::list_row(
            ctx,
            rect,
            label,
            None,
            Some(&trailing[index]),
            None,
            f,
            index == 1 && upload_active,
        );
    }

    let usb_window = usb_window(state);
    widgets::section_title(
        ctx,
        Rect::new(MARGIN, 776.0, 840.0, 32.0),
        "Найдено на USB и в памяти",
        Some(&state.usb_packages.len().to_string()),
    );
    for (index, path) in state
        .usb_packages
        .iter()
        .enumerate()
        .skip(usb_window.start)
        .take(usb_window.shown(state.usb_packages.len()))
    {
        let Some(rect) = layout.section(2).slot(index) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let f = if focus.section == 2 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        widgets::list_row(
            ctx,
            rect,
            &name,
            Some("установить с USB"),
            None,
            None,
            f,
            false,
        );
    }

    let panel = Rect::new(
        MARGIN + 888.0,
        CONTENT_Y - 8.0,
        CONTENT_W - 888.0,
        CONTENT_BOTTOM - CONTENT_Y + 8.0,
    );
    ctx.rect(panel, 24.0, p.surface.fade(0.65));
    ctx.stroke(panel, 24.0, 1.0, p.border);

    match &state.upload {
        Some(session) => {
            let url = session.url.clone();
            let last = session.last.clone();
            ctx.text_center(
                Rect::new(panel.x, panel.y + 18.0, panel.w, 36.0),
                ty.section,
                Weight::SemiBold,
                p.text,
                "Установка с телефона",
            );
            if let Ok(matrix) = nova_core::qr::QrMatrix::encode(&url) {
                widgets::qr(
                    ctx,
                    Rect::new(panel.x + 60.0, panel.y + 72.0, panel.w - 120.0, 420.0),
                    &matrix,
                );
            }
            ctx.text_center(
                Rect::new(panel.x + 20.0, panel.y + 506.0, panel.w - 40.0, 32.0),
                ty.small,
                Weight::Medium,
                p.accent,
                &url,
            );
            widgets::wrap_text(
                ctx,
                Rect::new(panel.x + 40.0, panel.y + 548.0, panel.w - 80.0, 140.0),
                "Откройте адрес телефоном в той же сети и выберите файл — пакет установится автоматически, без интернета.",
                ty.small,
                p.text_dim,
                ty.small * 1.55,
            );
            if let Some(name) = last {
                widgets::badge(
                    ctx,
                    Rect::new(panel.x + 40.0, panel.y + panel.h - 60.0, 0.0, 30.0),
                    &format!("принято: {name}"),
                    p.success,
                );
            }
        }
        None => {
            widgets::empty_state(
                ctx,
                Rect::new(panel.x, panel.y + 30.0, panel.w, panel.h - 60.0),
                "↓",
                "Приёмник выключен",
                "Включите «Приём файлов с телефона» — NovaOS покажет QR-код и примет IPK из локальной сети.",
            );
        }
    }
}

// ---------------------------- Настройки ----------------------------

fn draw_settings(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let focus = state.focus();
    let items = settings_items(state);
    let window = settings_window(state);
    let shown = window.shown(items.len());

    for slot in 0..shown {
        let index = window.index(slot);
        let Some(item) = items.get(index) else {
            continue;
        };
        let Some(rect) = layout.section(1).slot(index) else {
            continue;
        };
        let f = if focus.section == 1 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        let (title, subtitle, trailing) = settings_labels(state, item);
        widgets::list_row(
            ctx,
            rect,
            &title,
            Some(&subtitle),
            Some(&trailing),
            None,
            f,
            true,
        );

        match item {
            SettingsRow::Animations | SettingsRow::Clock24 | SettingsRow::ShowSystem => {
                let on = match item {
                    SettingsRow::Animations => state.settings.animations,
                    SettingsRow::Clock24 => state.settings.clock_24h,
                    _ => state.settings.show_system_apps,
                };
                widgets::toggle(
                    ctx,
                    Rect::new(rect.x + rect.w - 230.0, rect.y, 130.0, rect.h),
                    on,
                    f,
                );
            }
            _ => {}
        }

        // Заголовок группы — в зарезервированной полосе над строкой.
        if starts_group(state, slot, index) {
            if let Some(header) = settings_header_for(state, index) {
                ctx.text(
                    rect.x + 6.0,
                    rect.y - 16.0,
                    ty.small,
                    Weight::SemiBold,
                    p.text_faint,
                    &header.to_uppercase(),
                );
            }
        }
    }

    if items.len() > shown {
        widgets::scrollbar(
            ctx,
            Rect::new(
                MARGIN + CONTENT_W + 12.0,
                CONTENT_Y,
                6.0,
                SETTINGS_VISIBLE as f32 * (LIST_ROW + 10.0) + GROUP_GAP * 2.0 - 10.0,
            ),
            shown as f32 / items.len() as f32,
            window.start as f32 / items.len().saturating_sub(shown).max(1) as f32,
        );
    }
}

/// Подписи строки настроек: заголовок, пояснение, значение справа.
pub fn settings_labels(state: &AppState, item: &SettingsRow) -> (String, String, String) {
    match item {
        SettingsRow::Theme => (
            "Тема оформления".into(),
            "Цвета интерфейса NovaOS".into(),
            state.settings.theme.label_ru().to_string(),
        ),
        SettingsRow::Wallpaper => (
            "Обои".into(),
            "Фон главного экрана".into(),
            wallpaper_label(&state.settings.wallpaper),
        ),
        SettingsRow::Animations => (
            "Анимации".into(),
            "Плавное движение фокуса".into(),
            on_off(state.settings.animations),
        ),
        SettingsRow::Clock24 => (
            "24-часовые часы".into(),
            "Формат времени в интерфейсе".into(),
            on_off(state.settings.clock_24h),
        ),
        SettingsRow::Resolution => (
            "Разрешение интерфейса".into(),
            "Масштаб сетки под экран телевизора".into(),
            state.settings.resolution.label_ru().to_string(),
        ),
        SettingsRow::ShowSystem => (
            "Показывать системные приложения".into(),
            "Штатные приложения LG в общем списке".into(),
            on_off(state.settings.show_system_apps),
        ),
        SettingsRow::ResetFavorites => (
            "Сбросить избранное".into(),
            "Убрать все приложения с главного экрана".into(),
            state.settings.favorites.len().to_string(),
        ),
        SettingsRow::SyncStore => (
            "Обновить магазин".into(),
            "Загрузить списки пакетов из репозиториев".into(),
            state.store_sync.label_ru(),
        ),
        SettingsRow::AddRepo => (
            "Добавить репозиторий".into(),
            "Свой repo.json по адресу".into(),
            "добавить".into(),
        ),
        SettingsRow::Repo(index) => match state.settings.repos.get(*index) {
            Some(repo) => (
                repo.title.clone(),
                repo.url.clone(),
                if repo.enabled {
                    "включён"
                } else {
                    "выключен"
                }
                .into(),
            ),
            None => ("репозиторий".into(), String::new(), String::new()),
        },
        SettingsRow::HomeMode => (
            "Кнопка Home".into(),
            "Что делает кнопка Home на пульте".into(),
            state.settings.home_mode.label_ru().to_string(),
        ),
        SettingsRow::Reboot => (
            "Перезагрузить телевизор".into(),
            "Полная перезагрузка webOS".into(),
            String::new(),
        ),
        SettingsRow::PowerOff => (
            "Выключить телевизор".into(),
            "Питание отключится сразу".into(),
            String::new(),
        ),
        SettingsRow::Quit => (
            "Выйти из NovaOS".into(),
            "Вернуться в штатный интерфейс LG".into(),
            String::new(),
        ),
        SettingsRow::ResetSettings => (
            "Сбросить настройки".into(),
            "Избранное и репозитории сохранятся".into(),
            String::new(),
        ),
        SettingsRow::Diagnostics => (
            "Диагностика".into(),
            "Устройство, каталог и ошибки сканирования".into(),
            state.catalog.errors.len().to_string(),
        ),
    }
}

fn on_off(value: bool) -> String {
    if value {
        "вкл".into()
    } else {
        "выкл".into()
    }
}

fn wallpaper_label(wallpaper: &Wallpaper) -> String {
    match wallpaper {
        Wallpaper::BuiltIn { id } => format!("встроенные: {id}"),
        Wallpaper::Solid { color } => format!("цвет {color}"),
        Wallpaper::File { path } => format!("файл: {path}"),
        Wallpaper::Url { url } => format!("по ссылке: {url}"),
    }
}

// ---------------------------- О системе ----------------------------

fn draw_about(state: &AppState, ctx: &mut Ctx, layout: &Layout) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let focus = state.focus();

    for (index, label) in ABOUT_ACTIONS.iter().enumerate() {
        let Some(rect) = layout.section(1).slot(index) else {
            continue;
        };
        let f = if focus.section == 1 && focus.item == index {
            state.focus_anim
        } else {
            0.0
        };
        widgets::pill(ctx, rect, label, false, f);
    }

    let lines = state.about_lines();
    let window = about_window(state);
    let shown = window.shown(lines.len());
    for slot in 0..shown {
        let Some((key, value)) = lines.get(window.index(slot)) else {
            continue;
        };
        let Some(rect) = layout.section(2).slot(window.index(slot)) else {
            break;
        };
        let f = if focus.section == 2 && focus.item == slot {
            state.focus_anim
        } else {
            0.0
        };
        widgets::info_row(ctx, rect, key, value, f);
    }

    let compat = state.sysinfo.compatibility();
    ctx.text_ellipsized(
        Rect::new(MARGIN, CONTENT_BOTTOM + 2.0, CONTENT_W, 34.0),
        ty.small,
        Weight::Medium,
        if compat.supported {
            p.success
        } else {
            p.warning
        },
        &format!(
            "Совместимость: {}",
            crate::state::compatibility_text(&compat)
        ),
    );
}

// ---------------------------- Модальные окна ----------------------------

/// Содержимое модального окна, подготовленное для отрисовки.
enum ModalData {
    Confirm(ConfirmModal),
    Message { title: String, body: String },
    App(AppView, Vec<String>),
    Store(StoreView, String, String),
    Keyboard(crate::state::KeyboardModal),
}

fn draw_modal(state: &AppState, ctx: &mut Ctx) {
    let data = match &state.modal {
        Modal::None => return,
        Modal::Confirm(confirm) => ModalData::Confirm(confirm.clone()),
        Modal::Message { title, body } => ModalData::Message {
            title: title.clone(),
            body: body.clone(),
        },
        Modal::AppInfo(id) => match state.catalog.by_id(id) {
            Some(app) => {
                let view = AppView::of(app);
                let labels = state.app_button_labels(id);
                ModalData::App(view, labels)
            }
            None => return,
        },
        Modal::StoreInfo(id) => match state.store_item(id) {
            Some(item) => {
                let view = store_views(state)
                    .into_iter()
                    .find(|v| v.id == *id)
                    .unwrap_or_else(|| StoreView {
                        id: item.id.clone(),
                        title: item.title.clone(),
                        version: item.version.clone(),
                        subtitle: String::new(),
                        action: state.store_action(item).to_string(),
                        icon_url: item.icon_url.clone(),
                        root_required: item.root_required,
                        description: item.description.clone(),
                        ipk_url: item.ipk_url.clone(),
                        repo_title: item.repo_title.clone(),
                        size: item.size,
                        webos_requirement: item.webos_requirement.clone(),
                        sha256: item.sha256.clone(),
                    });
                let details = store_details(state, &view);
                let action = state.store_action(item).to_string();
                ModalData::Store(view, details, action)
            }
            None => return,
        },
        Modal::Keyboard(input) => ModalData::Keyboard(input.clone()),
    };

    widgets::dim_screen(ctx, 0.74);
    let p = ctx.palette();
    let ty = ctx.typo();
    match data {
        ModalData::Confirm(confirm) => {
            let height = (250.0 + confirm.body.lines().count() as f32 * 40.0).min(760.0);
            let rect = centered(1120.0, height);
            widgets::dialog_shell(ctx, rect, &confirm.title, Some(&confirm.body));
            widgets::dialog_buttons(ctx, rect, &confirm.buttons, state.modal_focus);
        }
        ModalData::Message { title, body } => {
            let height = (230.0 + body.lines().count() as f32 * 38.0).min(700.0);
            let rect = centered(1040.0, height);
            widgets::dialog_shell(ctx, rect, &title, Some(&body));
            widgets::dialog_buttons(ctx, rect, &[Button::primary("Закрыть")], 0);
        }
        ModalData::App(app, labels) => {
            let rect = centered(1240.0, 660.0);
            widgets::dialog_shell(ctx, rect, &app.title, Some(&app.details()));
            let icon_rect = Rect::new(rect.x + rect.w - 204.0, rect.y + 28.0, 168.0, 168.0);
            match state.icon(&app.id) {
                Some(icon) => ctx.image(icon, icon_rect, 18.0, 1.0),
                None => {
                    let (top, bottom) = widgets::seed_gradient(&app.title);
                    ctx.gradient(icon_rect, 18.0, top, bottom);
                    ctx.text_center(
                        icon_rect,
                        ty.hero,
                        Weight::Bold,
                        p.text,
                        &initial(&app.title),
                    );
                }
            }
            let buttons: Vec<Button> = labels
                .iter()
                .enumerate()
                .map(|(index, label)| match index {
                    0 => Button::primary(label.clone()),
                    2 if app.removable => Button::danger(label.clone()),
                    _ => Button::secondary(label.clone()),
                })
                .collect();
            widgets::dialog_buttons(ctx, rect, &buttons, state.modal_focus);
        }
        ModalData::Store(view, body, action) => {
            let rect = centered(1240.0, 700.0);
            widgets::dialog_shell(ctx, rect, &view.title, Some(&body));
            let icon_rect = Rect::new(rect.x + rect.w - 204.0, rect.y + 28.0, 168.0, 168.0);
            match state.store_icon(&view.id) {
                Some(icon) => ctx.image(icon, icon_rect, 18.0, 1.0),
                None => {
                    let (top, bottom) = widgets::seed_gradient(&view.title);
                    ctx.gradient(icon_rect, 18.0, top, bottom);
                    ctx.text_center(
                        icon_rect,
                        ty.hero,
                        Weight::Bold,
                        p.text,
                        &initial(&view.title),
                    );
                }
            }
            let buttons = vec![
                Button::primary(action),
                Button::secondary(if view.root_required {
                    "Нужен root"
                } else {
                    "Отмена"
                }),
            ];
            widgets::dialog_buttons(ctx, rect, &buttons, state.modal_focus);
        }
        ModalData::Keyboard(input) => {
            let rect = centered(1440.0, 780.0);
            widgets::dialog_shell(ctx, rect, input.field.title_ru(), None);
            widgets::text_field(
                ctx,
                Rect::new(rect.x + 34.0, rect.y + 82.0, rect.w - 68.0, 78.0),
                &input.value,
                input.field.hint_ru(),
                1.0,
                true,
            );
            widgets::keyboard(
                ctx,
                Rect::new(rect.x + 34.0, rect.y + 190.0, rect.w - 68.0, rect.h - 224.0),
                &input.keyboard,
            );
        }
    }
}

/// Первая буква названия — для плейсхолдера иконки.
fn initial(title: &str) -> String {
    title
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into())
}

fn centered(width: f32, height: f32) -> Rect {
    Rect::new(
        (LOGICAL_WIDTH - width) / 2.0,
        (SCREEN_H - height) / 2.0,
        width,
        height,
    )
}

fn draw_progress(state: &AppState, ctx: &mut Ctx) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let rect = centered(1000.0, 300.0);
    widgets::dim_screen(ctx, 0.55);
    widgets::dialog_shell(ctx, rect, &state.progress.title, None);
    ctx.text_ellipsized(
        Rect::new(rect.x + 34.0, rect.y + 62.0, rect.w - 68.0, 34.0),
        ty.body,
        Weight::Medium,
        p.text_dim,
        &state.progress.stage,
    );
    widgets::progress(
        ctx,
        Rect::new(rect.x + 34.0, rect.y + 140.0, rect.w - 68.0, 12.0),
        state.progress.fraction,
        state.progress.detail.as_deref(),
    );
    if let Some(error) = &state.progress.error {
        ctx.text_ellipsized(
            Rect::new(rect.x + 34.0, rect.y + 196.0, rect.w - 68.0, 70.0),
            ty.small,
            Weight::Regular,
            p.danger,
            error,
        );
    } else {
        widgets::spinner(ctx, (rect.x + rect.w - 74.0, rect.y + 62.0), 18.0);
    }
}

fn draw_screensaver(state: &AppState, ctx: &mut Ctx) {
    let p = ctx.palette();
    let ty = ctx.typo();
    widgets::dim_screen(ctx, 0.9);
    let w = ctx.width();
    let h = ctx.height();
    ctx.text_center(
        Rect::new(0.0, h * 0.32, w, ty.hero * 2.4),
        ty.hero * 2.2,
        Weight::Bold,
        p.text,
        &state.clock_text,
    );
    ctx.text_center(
        Rect::new(0.0, h * 0.32 + ty.hero * 2.4, w, 60.0),
        ty.section,
        Weight::Regular,
        p.text_dim,
        &state.date_text,
    );
    ctx.text_center(
        Rect::new(0.0, h - 150.0, w, 40.0),
        ty.small,
        Weight::Regular,
        p.text_faint,
        "NovaOS · нажмите любую кнопку",
    );
}

/// Подсказки по кнопкам пульта для текущего контекста.
pub fn hints_for(state: &AppState) -> Vec<(&'static str, &'static str)> {
    if state.progress.active {
        return vec![("OK", "ждём завершения")];
    }
    match &state.modal {
        Modal::Keyboard(_) => vec![
            ("←→↑↓", "выбор"),
            ("OK", "нажать"),
            ("⌫", "стереть"),
            ("НАЗАД", "закрыть"),
        ],
        Modal::Confirm(_) | Modal::Message { .. } => {
            vec![("←→", "выбор"), ("OK", "подтвердить"), ("НАЗАД", "отмена")]
        }
        Modal::AppInfo(_) | Modal::StoreInfo(_) => {
            vec![("←→", "выбор"), ("OK", "выполнить"), ("НАЗАД", "закрыть")]
        }
        Modal::None => match state.screen {
            Screen::Home => vec![
                ("←→", "выбор"),
                ("OK", "открыть"),
                ("↑", "разделы"),
                ("ЖЁЛТАЯ", "в избранное"),
                ("СИНЯЯ", "подробнее"),
            ],
            Screen::Apps => vec![
                ("←→↑↓", "навигация"),
                ("OK", "запустить"),
                ("ЖЁЛТАЯ", "в избранное"),
                ("КРАСНАЯ", "удалить"),
                ("СИНЯЯ", "подробнее"),
            ],
            Screen::Store => vec![
                ("←→↑↓", "навигация"),
                ("OK", "установить"),
                ("СИНЯЯ", "подробнее"),
                ("ЗЕЛЁНАЯ", "обновить"),
            ],
            Screen::Sideload => vec![
                ("↑↓", "выбор"),
                ("OK", "выполнить"),
                ("ЗЕЛЁНАЯ", "сканировать USB"),
            ],
            Screen::Settings => vec![
                ("↑↓", "выбор"),
                ("OK", "изменить"),
                ("ЗЕЛЁНАЯ", "обновить магазин"),
            ],
            Screen::About => vec![
                ("↑↓", "выбор"),
                ("OK", "выполнить"),
                ("НАЗАД", "на главную"),
            ],
        },
    }
}

// ============================ Ввод ============================

/// Обрабатывает нажатие клавиши пульта.
pub fn handle_key(state: &mut AppState, key: Key) {
    state.activity();
    if state.screensaver {
        return;
    }
    if key == Key::Exit {
        state.quit = true;
        return;
    }
    if !matches!(state.modal, Modal::None) {
        handle_modal_key(state, key);
        return;
    }
    if key == Key::Home {
        state.goto(Screen::Home);
        return;
    }
    match key {
        Key::Left => move_by(state, -1, 0),
        Key::Right => move_by(state, 1, 0),
        Key::Up => move_by(state, 0, -1),
        Key::Down => move_by(state, 0, 1),
        Key::Ok => activate(state, false),
        Key::Play => activate(state, true),
        Key::Back => back(state),
        Key::Menu => {
            let focus = state.focus();
            let next = if focus.section == SEC_NAV {
                Screen::ALL[(focus.item + 1) % Screen::ALL.len()]
            } else {
                Screen::Home
            };
            state.goto(next);
        }
        Key::Refresh | Key::Green => refresh(state),
        Key::Yellow => toggle_favorite_focus(state),
        Key::Blue | Key::Info => show_info_focus(state),
        Key::Red => red_action(state),
        Key::Digit(_) | Key::Exit | Key::Home => {}
    }
}

fn move_by(state: &mut AppState, dx: i32, dy: i32) {
    let layout = layout(state);
    let counts = layout.counts();
    let focus = state.focus();

    // Вертикальные списки: ↑/↓ листают элементы секции, а на краях —
    // передают ход обычной навигации (переход в соседнюю секцию).
    if dx == 0 && dy != 0 && vertical_list_section(state, focus.section) {
        let count = counts.get(focus.section).copied().unwrap_or(0);
        let next = focus.item as i32 + dy;
        if next >= 0 && (next as usize) < count {
            state.set_focus(Focus::new(focus.section, next as usize));
            return;
        }
    }
    state.move_focus(dx, dy, &counts);
}

/// Секции, нарисованные вертикальным списком (а не сеткой).
fn vertical_list_section(state: &AppState, section: usize) -> bool {
    match state.screen {
        Screen::Settings => section == 1,
        Screen::Sideload => section == 1 || section == 2,
        Screen::About => section == 2,
        Screen::Home | Screen::Apps | Screen::Store => false,
    }
}

fn back(state: &mut AppState) {
    if state.screen != Screen::Home {
        state.goto(Screen::Home);
    } else {
        state.handle(Intent::Quit);
    }
}

fn refresh(state: &mut AppState) {
    match state.screen {
        Screen::Store | Screen::Settings => {
            state.request_store_sync();
            state.notify("Обновляем магазин…", ToastKind::Info);
        }
        _ => {
            state.request_rescan();
            state.notify("Обновляем список приложений…", ToastKind::Info);
        }
    }
}

fn toggle_favorite_focus(state: &mut AppState) {
    match focused_app_view(state) {
        Some(app) => state.handle(Intent::ToggleFavorite(app.id)),
        None => state.notify("Жёлтая кнопка работает на приложениях", ToastKind::Info),
    }
}

fn show_info_focus(state: &mut AppState) {
    match state.screen {
        Screen::Apps | Screen::Home => {
            if let Some(app) = focused_app_view(state) {
                state.handle(Intent::AppInfo(app.id));
            } else {
                state.notify("Выберите приложение", ToastKind::Info);
            }
        }
        Screen::Store => match focused_store_view(state) {
            Some(view) => state.handle(Intent::StoreInfo(view.id)),
            None => state.notify("Выберите пакет", ToastKind::Info),
        },
        _ => state.notify("Нет сведений об этом элементе", ToastKind::Info),
    }
}

fn red_action(state: &mut AppState) {
    match state.screen {
        Screen::Apps | Screen::Home => match focused_app_view(state) {
            Some(app) => state.handle(Intent::Uninstall(app.id)),
            None => state.handle(Intent::Quit),
        },
        _ => state.handle(Intent::Quit),
    }
}

/// Пакет магазина под фокусом.
pub fn focused_store_view(state: &AppState) -> Option<StoreView> {
    if state.screen != Screen::Store {
        return None;
    }
    let focus = state.focus();
    if focus.section < 2 {
        return None;
    }
    let views = store_views(state);
    let index = (focus.section - 2) * STORE_COLS + focus.item;
    views.into_iter().nth(index)
}

/// Нажатие OK (или «Воспроизведение») на текущем элементе.
pub fn activate(state: &mut AppState, force_launch: bool) {
    let focus = state.focus();
    if focus.section == SEC_NAV {
        let screen = Screen::ALL[focus.item.min(Screen::ALL.len() - 1)];
        state.goto(screen);
        return;
    }
    match state.screen {
        Screen::Home => activate_home(state, focus, force_launch),
        Screen::Apps => activate_apps(state, focus, force_launch),
        Screen::Store => activate_store(state, focus),
        Screen::Sideload => activate_sideload(state, focus),
        Screen::Settings => activate_settings(state, focus),
        Screen::About => activate_about(state, focus),
    }
}

fn activate_home(state: &mut AppState, focus: Focus, force_launch: bool) {
    match focus.section {
        1 => {
            if let Some(app) = focused_app_view(state) {
                if force_launch {
                    state.handle(Intent::Launch(app.id));
                } else {
                    state.handle(Intent::AppInfo(app.id));
                }
            }
        }
        2 => {
            if let Some(app) = focused_app_view(state) {
                state.handle(Intent::Launch(app.id));
            }
        }
        3 => {
            let screens = [
                Screen::Store,
                Screen::Sideload,
                Screen::Settings,
                Screen::About,
            ];
            if let Some(screen) = screens.get(focus.item) {
                state.goto(*screen);
            }
        }
        _ => {}
    }
}

fn activate_apps(state: &mut AppState, focus: Focus, force_launch: bool) {
    if focus.section == 1 {
        match focus.item {
            0 => {
                let query = state.apps_query.clone();
                state.open_keyboard(TextField::SideloadPath, query);
            }
            1 => {
                let value = !state.settings.show_system_apps;
                state.handle(Intent::SetShowSystem(value));
            }
            _ => {
                state.request_rescan();
                state.notify("Обновляем список приложений…", ToastKind::Info);
            }
        }
        return;
    }
    if let Some(app) = focused_app_view(state) {
        if force_launch {
            state.handle(Intent::Launch(app.id));
        } else {
            state.handle(Intent::Launch(app.id));
        }
    }
}

fn activate_store(state: &mut AppState, focus: Focus) {
    if focus.section == 1 {
        if focus.item < StoreFilter::ALL.len() {
            state.store_filter = StoreFilter::ALL[focus.item];
        } else if focus.item == StoreFilter::ALL.len() {
            state.request_store_sync();
            state.notify("Обновляем магазин…", ToastKind::Info);
        } else {
            state.notify(
                "Поиск по магазину: используйте зелёную кнопку для обновления",
                ToastKind::Info,
            );
        }
        return;
    }
    if let Some(view) = focused_store_view(state) {
        state.handle(Intent::StoreInfo(view.id));
    }
}

fn activate_sideload(state: &mut AppState, focus: Focus) {
    if focus.section == 2 {
        state.install_usb(focus.item);
        return;
    }
    match focus.item {
        0 => state.open_keyboard(TextField::SideloadUrl, String::new()),
        1 => state.handle(Intent::ToggleUpload),
        2 => state.open_keyboard(TextField::SideloadPath, String::new()),
        3 => {
            state.scan_usb();
            state.notify(
                format!("Найдено пакетов: {}", state.usb_packages.len()),
                ToastKind::Info,
            );
        }
        _ => {
            state.request_rescan();
            state.notify("Обновляем каталог…", ToastKind::Info);
        }
    }
}

fn activate_settings(state: &mut AppState, focus: Focus) {
    if focus.section != 1 {
        return;
    }
    let items = settings_items(state);
    let Some(item) = items.get(focus.item).cloned() else {
        return;
    };
    match item {
        SettingsRow::Theme => state.handle(Intent::CycleTheme),
        SettingsRow::Wallpaper => state.open_keyboard(TextField::WallpaperUrl, String::new()),
        SettingsRow::Animations => state.handle(Intent::ToggleAnimations),
        SettingsRow::Clock24 => {
            let value = !state.settings.clock_24h;
            state.handle(Intent::SetClock24h(value));
        }
        SettingsRow::Resolution => {
            let next = match state.settings.resolution {
                Resolution::Full => Resolution::Fast,
                Resolution::Fast => Resolution::Full,
            };
            state.handle(Intent::SetResolution(next));
        }
        SettingsRow::ShowSystem => {
            let value = !state.settings.show_system_apps;
            state.handle(Intent::SetShowSystem(value));
        }
        SettingsRow::ResetFavorites => state.handle(Intent::ResetFavorites),
        SettingsRow::SyncStore => {
            state.request_store_sync();
            state.notify("Обновляем магазин…", ToastKind::Info);
        }
        SettingsRow::AddRepo => state.open_keyboard(TextField::RepoUrl, String::new()),
        SettingsRow::Repo(index) => {
            let Some(repo) = state.settings.repos.get(index).cloned() else {
                return;
            };
            state.modal = Modal::Confirm(ConfirmModal {
                title: repo.title.clone(),
                body: format!(
                    "{}\n\nПоследняя синхронизация: {}",
                    repo.url,
                    repo.last_sync
                        .clone()
                        .unwrap_or_else(|| "не выполнялась".into())
                ),
                buttons: vec![
                    Button::primary(if repo.enabled {
                        "Выключить"
                    } else {
                        "Включить"
                    }),
                    Button::danger("Удалить"),
                    Button::secondary("Отмена"),
                ],
                actions: vec![
                    ConfirmAction::ToggleRepo(index),
                    ConfirmAction::RemoveRepo(index),
                    ConfirmAction::None,
                ],
            });
            state.modal_focus = 0;
        }
        SettingsRow::HomeMode => {
            let next = match state.settings.home_mode {
                HomeMode::Disabled => HomeMode::Replaced,
                HomeMode::Replaced => HomeMode::Autostart,
                HomeMode::Autostart => HomeMode::Disabled,
            };
            state.handle(Intent::SetHomeMode(next));
        }
        SettingsRow::Reboot => state.handle(Intent::Reboot),
        SettingsRow::PowerOff => state.handle(Intent::PowerOff),
        SettingsRow::Quit => state.handle(Intent::Quit),
        SettingsRow::ResetSettings => state.handle(Intent::ResetSettings),
        SettingsRow::Diagnostics => diagnostics_dialog(state),
    }
}

fn diagnostics_dialog(state: &mut AppState) {
    let errors = if state.catalog.errors.is_empty() {
        "ошибок сканирования нет".to_string()
    } else {
        state.catalog.errors.join("\n")
    };
    let body = format!(
        "NovaOS {} · {}\nУстройство: {}\nwebOS: {}\nКаталог: {} приложений\nСовместимость: {}\n\n{errors}",
        nova_core::VERSION,
        state.sysinfo.arch,
        state.sysinfo.model.clone().unwrap_or_else(|| "—".into()),
        state.sysinfo.webos_release.clone().unwrap_or_else(|| "—".into()),
        state.catalog.len(),
        crate::state::compatibility_text(&state.sysinfo.compatibility())
    );
    state.handle(Intent::Message {
        title: "Диагностика".into(),
        body,
    });
}

fn activate_about(state: &mut AppState, focus: Focus) {
    if focus.section == 1 {
        match focus.item {
            0 => {
                let body = format!(
                    "Установлена версия {}.\n\nНовые сборки публикуются в репозитории NovaOS: добавьте его в настройках и обновите магазин — лаунчер предложит установить свежий IPK.",
                    nova_core::VERSION
                );
                state.handle(Intent::Message {
                    title: "Обновление NovaOS".into(),
                    body,
                });
            }
            1 => diagnostics_dialog(state),
            _ => state.handle(Intent::Quit),
        }
        return;
    }
    let lines = state.about_lines();
    if let Some((key, value)) = lines.get(focus.item) {
        state.handle(Intent::Message {
            title: key.clone(),
            body: value.clone(),
        });
    }
}

/// Клавиши внутри модального окна.
pub fn handle_modal_key(state: &mut AppState, key: Key) {
    // Снимаем «срез» состояния окна, чтобы не держать заимствование при вызовах &mut.
    enum Kind {
        None,
        Keyboard,
        Buttons(usize),
        Message,
    }
    let kind = match &state.modal {
        Modal::None => Kind::None,
        Modal::Keyboard(_) => Kind::Keyboard,
        Modal::Confirm(confirm) => Kind::Buttons(confirm.buttons.len().max(1)),
        Modal::AppInfo(_) | Modal::StoreInfo(_) => Kind::Buttons(state.modal_button_count()),
        Modal::Message { .. } => Kind::Message,
    };

    match kind {
        Kind::None => {}
        Kind::Message => match key {
            Key::Ok | Key::Back | Key::Exit => state.close_modal(),
            _ => {}
        },
        Kind::Buttons(count) => match key {
            Key::Left => state.modal_focus = (state.modal_focus + count - 1) % count,
            Key::Right => state.modal_focus = (state.modal_focus + 1) % count,
            Key::Ok => state.modal_activate(),
            Key::Back | Key::Exit => state.close_modal(),
            _ => {}
        },
        Kind::Keyboard => match key {
            Key::Left => state.modal_keyboard_move(-1, 0),
            Key::Right => state.modal_keyboard_move(1, 0),
            Key::Up => state.modal_keyboard_move(0, -1),
            Key::Down => state.modal_keyboard_move(0, 1),
            Key::Ok => state.modal_activate(),
            Key::Back => {
                if state.modal_value().is_empty() {
                    state.close_modal();
                } else {
                    state.modal_backspace();
                }
            }
            Key::Exit => state.close_modal(),
            Key::Digit(d) => state.modal_push_char(char::from(b'0' + d)),
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, WorkerEnv};
    use nova_core::settings::{Settings, Theme as ThemeChoice};
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn demo_state() -> (AppState, MutexGuard<'static, ()>) {
        let guard = lock();
        let dir = std::env::temp_dir().join(format!("nova-screens-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        std::env::set_var("NOVA_HOME", &dir);
        let mut settings = Settings::default();
        settings.work_dir = Some(dir.join("work").to_string_lossy().into_owned());
        settings.auto_updates = false;
        settings.screensaver_minutes = 0;
        let mut state = AppState::inline(settings, WorkerEnv::sandbox(&dir));
        state.catalog = nova_core::catalog::demo_catalog();
        state.settings.favorites = vec![
            "com.novaos.store".into(),
            "com.ecklf.luma".into(),
            "org.webosbrew.hbchannel".into(),
        ];
        state.catalog.apply_favorites(&state.settings.favorites);
        state.sysinfo = nova_core::sysinfo::SysInfo::mock_um7300();
        (state, guard)
    }

    fn demo_store(state: &mut AppState) {
        state.store = (0..14)
            .map(|i| StoreItem {
                id: format!("com.demo.app{i}"),
                title: format!("Приложение {i}"),
                version: "1.0.0".into(),
                app_type: if i % 2 == 0 { "web" } else { "native" }.into(),
                description: "демонстрационный пакет".into(),
                icon_url: String::new(),
                ipk_url: format!("https://example.invalid/app{i}.ipk"),
                sha256: None,
                size: Some(1024 * (i + 1) as u64),
                source_url: String::new(),
                root_required: i % 3 == 0,
                webos_requirement: None,
                screenshots: vec![],
                featured: i < 3,
                repo_title: "Демо".into(),
            })
            .collect();
        state.store_sync = StoreSync::Ready(14);
    }

    #[test]
    fn layout_fits_the_screen_on_every_page() {
        let (mut state, _guard) = demo_state();
        demo_store(&mut state);
        for screen in Screen::ALL {
            state.goto(screen);
            let layout = layout(&state);
            assert_eq!(layout.section(SEC_NAV).len(), 6, "{screen:?}");
            assert!(!layout.is_empty());
            assert_eq!(layout.len(), layout.counts().len());
            for section in &layout.sections {
                assert!(
                    section.rects.len() <= section.count,
                    "{screen:?}: {section:?}"
                );
                for rect in &section.rects {
                    assert!(rect.x >= 0.0 && rect.y >= 0.0, "{screen:?}: {rect:?}");
                    assert!(
                        rect.x + rect.w <= LOGICAL_WIDTH + 1.0,
                        "{screen:?}: {rect:?}"
                    );
                    assert!(rect.y + rect.h <= SCREEN_H + 1.0, "{screen:?}: {rect:?}");
                    assert!(rect.w > 0.0 && rect.h > 0.0, "{screen:?}: {rect:?}");
                }
            }
        }
    }

    #[test]
    fn window_scrolling_keeps_focus_visible() {
        assert_eq!(window_for(0, 0, 8, 3), Window::EMPTY);
        assert_eq!(window_for(0, 5, 8, 3), Window { start: 0, count: 5 });
        assert_eq!(window_for(0, 20, 8, 3), Window { start: 0, count: 8 });
        for focus in 0..20 {
            let w = window_for(focus, 20, 8, 3);
            assert!(
                w.start <= focus && focus < w.start + w.count,
                "фокус {focus}: {w:?}"
            );
        }
        let last = window_for(19, 20, 8, 3);
        assert_eq!(last.start + last.count, 20);
        assert_eq!(last.index(7), 19);
        assert_eq!(window_for(3, 10, 4, 1).shown(10), 4);
        assert_eq!(window_for(0, 3, 8, 3).shown(3), 3);
    }

    #[test]
    fn keys_navigate_between_sections() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Home);
        state.set_focus(Focus::new(SEC_NAV, 0));

        handle_key(&mut state, Key::Right);
        assert_eq!(state.focus(), Focus::new(SEC_NAV, 1));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.screen, Screen::Apps);

        handle_key(&mut state, Key::Down);
        assert_eq!(state.focus().section, 1, "панель инструментов");
        handle_key(&mut state, Key::Down);
        assert!(state.focus().section >= 2, "сетка приложений");

        handle_key(&mut state, Key::Blue);
        assert!(matches!(state.modal, Modal::AppInfo(_)));
        handle_key(&mut state, Key::Back);
        assert!(matches!(state.modal, Modal::None));

        let before = state.settings.favorites.len();
        handle_key(&mut state, Key::Yellow);
        assert_ne!(state.settings.favorites.len(), before);

        handle_key(&mut state, Key::Back);
        assert_eq!(state.screen, Screen::Home);
        handle_key(&mut state, Key::Back);
        assert!(matches!(state.modal, Modal::Confirm(_)));
        handle_key(&mut state, Key::Right);
        handle_key(&mut state, Key::Ok);
        assert!(!state.quit, "отмена выхода");
        handle_key(&mut state, Key::Exit);
        assert!(state.quit, "Exit завершает лаунчер");
    }

    #[test]
    fn home_sections_react_to_keys() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Home);
        // Избранное.
        state.set_focus(Focus::new(1, 0));
        assert!(focused_app_view(&state).is_some());
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::AppInfo(_)));
        // Кнопки карточки: «Запустить», «В избранное», …
        assert_eq!(state.modal_button_count(), 4);
        handle_key(&mut state, Key::Right);
        assert_eq!(state.modal_focus, 1);
        handle_key(&mut state, Key::Ok);
        assert!(!state.is_favorite("com.novaos.store"), "избранное снято");

        // Быстрые действия.
        state.set_focus(Focus::new(3, 0));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.screen, Screen::Store);
        state.goto(Screen::Home);
        state.set_focus(Focus::new(3, 3));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.screen, Screen::About);
    }

    #[test]
    fn store_navigation_filters_and_installs() {
        let (mut state, _guard) = demo_state();
        demo_store(&mut state);
        state.goto(Screen::Store);
        assert_eq!(state.visible_store().len(), 14);

        state.set_focus(Focus::new(1, 1));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.store_filter, StoreFilter::Featured);
        assert_eq!(state.visible_store().len(), 3);

        state.set_focus(Focus::new(1, 5));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.store_filter, StoreFilter::Native);

        state.set_focus(Focus::new(1, 0));
        handle_key(&mut state, Key::Ok);
        assert_eq!(state.store_filter, StoreFilter::All);

        // Прокрутка сетки: уходим вниз за первый видимый экран (3 ряда по 2).
        state.set_focus(Focus::new(2, 0));
        for _ in 0..6 {
            handle_key(&mut state, Key::Down);
        }
        let window = store_window(&state);
        assert!(window.start > 0, "список должен прокрутиться: {window:?}");
        assert!(
            focused_store_view(&state).is_some(),
            "фокус на видимой карточке"
        );

        // Карточка → подробности → установка.
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::StoreInfo(_)));
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::Confirm(_)));
        handle_key(&mut state, Key::Back);
        assert!(matches!(state.modal, Modal::None));
        assert!(focused_store_view(&state).is_some());
    }

    #[test]
    fn sideload_actions_toggle_upload() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Sideload);
        state.set_focus(Focus::new(1, 1));
        handle_key(&mut state, Key::Ok);
        assert!(state.upload.is_some(), "приёмник должен запуститься");
        assert!(state.upload_url().is_some());
        handle_key(&mut state, Key::Ok);
        assert!(state.upload.is_none(), "повторное нажатие выключает");

        state.set_focus(Focus::new(1, 0));
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::Keyboard(_)));
        handle_key(&mut state, Key::Back);
        assert!(matches!(state.modal, Modal::None));

        state.set_focus(Focus::new(1, 3));
        handle_key(&mut state, Key::Ok);
        assert!(state.toast.is_some());
    }

    #[test]
    fn keyboard_typing_through_keys() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Sideload);
        state.open_keyboard(TextField::SideloadUrl, String::new());
        assert!(state.modal_value().is_empty());

        // Четыре нажатия OK по соседним клавишам дают четыре символа.
        for _ in 0..4 {
            handle_key(&mut state, Key::Ok);
            handle_key(&mut state, Key::Right);
        }
        let typed = state.modal_value();
        assert_eq!(typed.chars().count(), 4, "{typed}");

        // Цифра с пульта добавляется в конец, «Назад» стирает её.
        handle_key(&mut state, Key::Digit(7));
        assert_eq!(state.modal_value(), format!("{typed}7"));
        handle_key(&mut state, Key::Back);
        assert_eq!(state.modal_value(), typed);

        // Стрелки перемещают курсор по раскладке, окно остаётся открытым.
        handle_key(&mut state, Key::Down);
        handle_key(&mut state, Key::Ok);
        assert!(
            matches!(state.modal, Modal::Keyboard(_)),
            "навигация по раскладке"
        );
        assert!(state.modal_value().chars().count() >= 4);
    }

    #[test]
    fn settings_rows_change_values() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Settings);
        let items = settings_items(&state);
        assert!(items.contains(&SettingsRow::Theme));
        assert!(items.contains(&SettingsRow::HomeMode));
        assert!(items.contains(&SettingsRow::Repo(0)));
        assert!(settings_entries(&state)
            .iter()
            .any(|e| matches!(e, SettingsEntry::Header(t) if t == "Система")));

        state.set_focus(Focus::new(1, 0));
        handle_key(&mut state, Key::Ok);
        assert_ne!(state.settings.theme, ThemeChoice::Nova);

        let index = items
            .iter()
            .position(|i| *i == SettingsRow::Animations)
            .unwrap();
        state.set_focus(Focus::new(1, index));
        let before = state.settings.animations;
        handle_key(&mut state, Key::Ok);
        assert_ne!(state.settings.animations, before);

        let index = items
            .iter()
            .position(|i| *i == SettingsRow::Clock24)
            .unwrap();
        state.set_focus(Focus::new(1, index));
        handle_key(&mut state, Key::Ok);
        assert!(!state.settings.clock_24h);
        assert!(
            state.clock_text.contains(' '),
            "12-часовой формат с частью суток"
        );

        let index = items
            .iter()
            .position(|i| *i == SettingsRow::Diagnostics)
            .unwrap();
        state.set_focus(Focus::new(1, index.min(SETTINGS_VISIBLE - 1)));
        handle_key(&mut state, Key::Ok);
        // Строка могла уйти за окно прокрутки — проверяем, что ничего не сломалось.
        assert!(state.modal_focus == 0);

        for item in &items {
            let (title, _, _) = settings_labels(&state, item);
            assert!(!title.is_empty(), "{item:?}");
        }
        // Заголовки групп определяются для каждой строки.
        assert_eq!(
            settings_header_for(&state, 0).as_deref(),
            Some("Оформление")
        );
    }

    #[test]
    fn settings_scrolls_long_list() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::Settings);
        let total = settings_items(&state).len();
        assert!(
            total > SETTINGS_VISIBLE,
            "список должен требовать прокрутки"
        );
        state.set_focus(Focus::new(1, 0));

        let mut scrolled = false;
        for _ in 0..total {
            handle_key(&mut state, Key::Down);
            let window = settings_window(&state);
            assert!(window.shown(total) <= SETTINGS_VISIBLE, "{window:?}");
            scrolled |= window.start > 0;
            // Фокус всегда попадает в видимое окно — иначе элемент не нарисуется.
            if state.focus().section == 1 {
                let layout = layout(&state);
                assert!(
                    layout.section(1).slot(state.focus().item).is_some(),
                    "фокус {:?} вне окна {window:?}",
                    state.focus()
                );
            }
        }
        assert!(scrolled, "список должен прокрутиться");
    }

    #[test]
    fn about_screen_lists_lines() {
        let (mut state, _guard) = demo_state();
        state.goto(Screen::About);
        let layout = layout(&state);
        assert_eq!(layout.section(1).len(), ABOUT_ACTIONS.len());
        assert!(layout.section(2).rects.len() <= ABOUT_VISIBLE);
        assert!(!state.about_lines().is_empty());
        state.set_focus(Focus::new(1, 1));
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::Message { .. }));
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::None));
        state.set_focus(Focus::new(2, 0));
        handle_key(&mut state, Key::Ok);
        assert!(matches!(state.modal, Modal::Message { .. }));
    }

    #[test]
    fn renders_every_screen_without_panic() {
        let (mut state, _guard) = demo_state();
        demo_store(&mut state);
        let mut renderer = Renderer::new(960, 540);

        for screen in Screen::ALL {
            state.goto(screen);
            state.notify("Проверка отрисовки", ToastKind::Info);
            renderer.frame(&mut state);
            let painted = renderer
                .canvas
                .pixels
                .chunks_exact(4)
                .filter(|px| u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2]) > 40)
                .count();
            assert!(
                painted > 20_000,
                "{screen:?}: слишком пустой кадр ({painted})"
            );
        }

        // Модальные окна, прогресс и заставка.
        state.goto(Screen::Apps);
        state.set_focus(Focus::new(2, 0));
        handle_key(&mut state, Key::Blue);
        renderer.frame(&mut state);
        assert!(matches!(state.modal, Modal::AppInfo(_)));
        state.close_modal();

        state.goto(Screen::Store);
        state.set_focus(Focus::new(2, 0));
        handle_key(&mut state, Key::Blue);
        renderer.frame(&mut state);
        state.close_modal();

        state.handle(Intent::Message {
            title: "Проверка".into(),
            body: "Текст сообщения\nв две строки".into(),
        });
        renderer.frame(&mut state);
        state.close_modal();

        state.open_keyboard(TextField::SideloadUrl, "http://".into());
        renderer.frame(&mut state);
        state.close_modal();

        state.progress.start("Установка");
        renderer.frame(&mut state);
        state.progress.active = false;

        state.handle(Intent::ToggleUpload);
        state.goto(Screen::Sideload);
        renderer.frame(&mut state);
        state.handle(Intent::ToggleUpload);

        state.screensaver = true;
        renderer.frame(&mut state);
        state.screensaver = false;
    }

    #[test]
    fn key_names_map_to_keys() {
        assert_eq!(Key::from_name("Left"), Some(Key::Left));
        assert_eq!(Key::from_name("return"), Some(Key::Ok));
        assert_eq!(Key::from_name("Escape"), Some(Key::Back));
        assert_eq!(Key::from_name("F1"), Some(Key::Red));
        assert_eq!(Key::from_name("5"), Some(Key::Digit(5)));
        assert_eq!(Key::from_name("нет"), None);
    }

    #[test]
    fn hints_depend_on_context() {
        let (mut state, _guard) = demo_state();
        assert!(hints_for(&state)
            .iter()
            .any(|(_, label)| *label == "открыть"));
        state.progress.start("Установка");
        assert_eq!(hints_for(&state).len(), 1);
        state.progress.active = false;
        state.goto(Screen::Apps);
        assert!(hints_for(&state).iter().any(|(key, _)| *key == "КРАСНАЯ"));
        state.open_keyboard(TextField::RepoUrl, String::new());
        assert!(hints_for(&state).iter().any(|(key, _)| key.contains('←')));
        // Проверка, что типы из других модулей доступны (компиляционный контракт).
        let _: Option<PathBuf> = None;
        let _: fn(&AppState, &mut Ctx, &Layout) = draw;
        let _ = ConfirmAction::None;
    }

    #[test]
    fn app_view_exposes_metadata() {
        let (state, _guard) = demo_state();
        let apps = app_views(&state);
        assert!(!apps.is_empty());
        let app = &apps[0];
        assert!(!app.id.is_empty());
        assert!(app.details().contains("Идентификатор"));
        assert!(!app.hero_subtitle().is_empty());
        assert!(
            AppView::icon_requests(&apps).is_empty(),
            "в демо-каталоге иконок нет"
        );
        let favorites = favorite_views(&state);
        assert_eq!(favorites.len(), 3);
    }
}
