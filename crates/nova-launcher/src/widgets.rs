//! Виджеты интерфейса NovaOS.
//!
//! Все примитивы работают в логической сетке 1920×1080; [`Ctx`] переводит их
//! в физические пиксели ровно один раз — на границе с холстом. Экраны
//! собирают виджеты как конструктор и не думают о масштабе.

use nova_core::qr::QrMatrix;

use crate::gfx::{Canvas, Color, FontSet, Image, Rect, TextCache, Weight};
use crate::theme::{Metrics, Palette, Theme, Typography, LOGICAL_WIDTH};

/// Контекст рисования: холст, шрифты, кэш глифов, тема, масштаб и время кадра.
pub struct Ctx<'a> {
    /// Холст.
    pub canvas: &'a mut Canvas,
    /// Набор шрифтов.
    pub fonts: &'a FontSet,
    /// Кэш глифов.
    pub cache: &'a mut TextCache,
    /// Тема оформления (копия: темы компактны и копируемы).
    pub theme: Theme,
    /// Масштаб (физические пиксели / логические).
    pub scale: f32,
    /// Время кадра в секундах (для анимаций).
    pub now: f32,
}

impl<'a> Ctx<'a> {
    /// Создаёт контекст; масштаб выводится из ширины холста.
    pub fn new(
        canvas: &'a mut Canvas,
        fonts: &'a FontSet,
        cache: &'a mut TextCache,
        theme: Theme,
        now: f32,
    ) -> Self {
        let scale = canvas.width as f32 / LOGICAL_WIDTH;
        Self {
            canvas,
            fonts,
            cache,
            theme,
            scale,
            now,
        }
    }

    /// Логическая величина в физические пиксели.
    #[inline]
    pub fn s(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// Логический прямоугольник в физический.
    #[inline]
    pub fn px(&self, r: Rect) -> Rect {
        Rect::new(
            r.x * self.scale,
            r.y * self.scale,
            r.w * self.scale,
            r.h * self.scale,
        )
    }

    /// Логическая ширина холста.
    pub fn width(&self) -> f32 {
        self.canvas.width as f32 / self.scale
    }

    /// Логическая высота холста.
    pub fn height(&self) -> f32 {
        self.canvas.height as f32 / self.scale
    }

    /// Прямоугольник всей области экрана в логических единицах.
    pub fn full(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width(), self.height())
    }

    pub fn palette(&self) -> Palette {
        self.theme.palette
    }

    pub fn metrics(&self) -> Metrics {
        self.theme.metrics
    }

    pub fn typo(&self) -> Typography {
        self.theme.typography
    }

    /// Скруглённая заливка.
    pub fn rect(&mut self, r: Rect, radius: f32, color: Color) {
        let s = self.scale;
        self.canvas.fill_rounded(self.px(r), radius * s, color);
    }

    /// Скруглённая обводка.
    pub fn stroke(&mut self, r: Rect, radius: f32, thickness: f32, color: Color) {
        let s = self.scale;
        self.canvas
            .stroke_rounded(self.px(r), radius * s, thickness * s, color);
    }

    /// Вертикальный градиент со скруглением.
    pub fn gradient(&mut self, r: Rect, radius: f32, top: Color, bottom: Color) {
        let s = self.scale;
        self.canvas
            .gradient_rounded(self.px(r), radius * s, top, bottom);
    }

    /// Тень под элементом.
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, offset_y: f32, color: Color) {
        let s = self.scale;
        self.canvas
            .shadow(self.px(r), radius * s, blur * s, offset_y * s, color);
    }

    /// Радиальное свечение (ореол фокуса).
    pub fn glow(&mut self, center: (f32, f32), radius: f32, color: Color) {
        let s = self.scale;
        self.canvas
            .radial_glow((center.0 * s, center.1 * s), radius * s, color);
    }

    /// Рисунок в прямоугольник.
    pub fn image(&mut self, img: &Image, r: Rect, radius: f32, alpha: f32) {
        let s = self.scale;
        self.canvas.blit(img, self.px(r), alpha, Some(radius * s));
    }

    /// Текст в логических координатах (`baseline_y` — базовая линия).
    pub fn text(
        &mut self,
        x: f32,
        baseline_y: f32,
        size: f32,
        weight: Weight,
        color: Color,
        text: &str,
    ) -> f32 {
        let Ctx {
            canvas,
            fonts,
            cache,
            scale,
            ..
        } = self;
        canvas.text(
            fonts,
            cache,
            x * *scale,
            baseline_y * *scale,
            size * *scale,
            weight,
            color,
            text,
        ) / *scale
    }

    /// Ширина текста в логических единицах.
    pub fn measure(&mut self, size: f32, weight: Weight, text: &str) -> f32 {
        let Ctx {
            fonts,
            cache,
            scale,
            ..
        } = self;
        Canvas::measure(fonts, cache, size * *scale, weight, text) / *scale
    }

    /// Текст, выровненный по центру прямоугольника.
    pub fn text_center(&mut self, r: Rect, size: f32, weight: Weight, color: Color, text: &str) {
        let tw = self.measure(size, weight, text);
        let x = r.x + (r.w - tw) / 2.0;
        let baseline = r.y + r.h / 2.0 + size * 0.36;
        self.text(x, baseline, size, weight, color, text);
    }

    /// Текст, обрезанный по ширине прямоугольника.
    pub fn text_ellipsized(
        &mut self,
        r: Rect,
        size: f32,
        weight: Weight,
        color: Color,
        text: &str,
    ) {
        let Ctx {
            canvas,
            fonts,
            cache,
            scale,
            ..
        } = self;
        let pr = Rect::new(r.x * *scale, r.y * *scale, r.w * *scale, r.h * *scale);
        canvas.text_ellipsis(
            fonts,
            cache,
            pr.x,
            pr.y + pr.h / 2.0 + size * *scale * 0.36,
            size * *scale,
            weight,
            color,
            text,
            pr.w,
        );
    }

    /// Очистка холста.
    pub fn clear(&mut self, color: Color) {
        self.canvas.clear(color);
    }
}

// ============================ Фон ============================

/// Фон экрана: обои или градиент, плюс затемнение сверху и снизу.
pub fn background(ctx: &mut Ctx, wallpaper: Option<&Image>) {
    let p = ctx.palette();
    let full = ctx.full();
    let w = full.w;
    let h = full.h;

    match wallpaper {
        Some(img) => ctx.image(img, full, 0.0, 1.0),
        None => ctx.gradient(full, 0.0, p.background, p.background_deep),
    }

    ctx.canvas.gradient_rect(
        ctx.px(Rect::new(0.0, 0.0, w, 170.0)),
        p.scrim_bottom.fade(0.35),
        p.scrim_top,
    );
    ctx.canvas.gradient_rect(
        ctx.px(Rect::new(0.0, h - 280.0, w, h)),
        p.scrim_top,
        p.scrim_bottom,
    );
}

/// Декоративные «пятна света» на фоне — оживляют пустой экран.
pub fn ambient(ctx: &mut Ctx) {
    let p = ctx.palette();
    let w = ctx.width();
    let h = ctx.height();
    ctx.glow((w * 0.18, h * 0.22), w * 0.28, p.accent.fade(0.10));
    ctx.glow((w * 0.86, h * 0.78), w * 0.30, p.accent_soft.fade(0.12));
}

// ============================ Верхняя панель ============================

/// Заголовок экрана, необязательный подзаголовок и часы справа.
pub fn top_bar(ctx: &mut Ctx, title: &str, clock: &str, subtitle: Option<&str>) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let m = ctx.metrics().margin;
    let baseline = m * 0.72 + ty.title * 0.36;
    ctx.text(m, baseline, ty.title, Weight::SemiBold, p.text, title);
    if let Some(s) = subtitle {
        ctx.text(
            m,
            baseline + ty.title * 0.62 + ty.body * 0.36,
            ty.body,
            Weight::Regular,
            p.text_dim,
            s,
        );
    }
    let cw = ctx.measure(ty.body, Weight::Medium, clock);
    ctx.text(
        ctx.width() - m - cw,
        baseline,
        ty.body,
        Weight::Medium,
        p.text_dim,
        clock,
    );
}

/// Ряд вкладок; возвращает прямоугольники вкладок.
pub fn tab_bar(ctx: &mut Ctx, rect: Rect, tabs: &[&str], active: usize, focus: f32) -> Vec<Rect> {
    let p = ctx.palette();
    let ty = ctx.typo();
    let mut out = Vec::with_capacity(tabs.len());
    let mut x = rect.x;
    for (i, tab) in tabs.iter().enumerate() {
        let w = ctx.measure(ty.body, Weight::Medium, tab) + 40.0;
        let item = Rect::new(x, rect.y, w, rect.h);
        out.push(item);
        if i == active {
            ctx.rect(item.inset(2.0), 12.0, p.accent.fade(0.16));
            ctx.text_center(item, ty.body, Weight::SemiBold, p.text, tab);
            ctx.rect(
                Rect::new(item.x + 18.0, item.y + item.h - 5.0, item.w - 36.0, 5.0),
                3.0,
                p.accent.mix(p.focus_glow, focus.clamp(0.0, 1.0)),
            );
        } else {
            ctx.text_center(item, ty.body, Weight::Medium, p.text_dim, tab);
        }
        x += w + 10.0;
    }
    out
}

// ============================ Плитки ============================

/// Состояние плитки приложения.
#[derive(Clone, Debug, Default)]
pub struct TileState {
    /// Фокус: 0 — нет, 1 — полный (плавно анимируется).
    pub focus: f32,
    /// Приложение запущено.
    pub running: bool,
    /// Подпись-бейдж в углу (например, «IPK», «web»).
    pub badge: Option<String>,
    /// Затемнить (скрытое системное приложение).
    pub dimmed: bool,
    /// Пометить как избранное.
    pub favorite: bool,
}

impl TileState {
    /// Плитка в фокусе.
    pub fn focused() -> Self {
        Self {
            focus: 1.0,
            ..Self::default()
        }
    }
}

/// Крупная карточка «Избранного» (арт 16:9).
pub fn hero_card(
    ctx: &mut Ctx,
    rect: Rect,
    title: &str,
    subtitle: &str,
    art: Option<&Image>,
    state: &TileState,
) {
    let p = ctx.palette();
    let m = ctx.metrics();
    let ty = ctx.typo();
    let f = state.focus.clamp(0.0, 1.0);
    let grown = rect.scaled_about_center(1.0 + (m.focus_scale - 1.0) * f);

    if f > 0.01 {
        ctx.glow(grown.center(), grown.w * 0.7, p.focus_glow.fade(0.30 * f));
    }
    ctx.shadow(
        grown,
        m.card_radius,
        18.0 + 26.0 * f,
        6.0 + 10.0 * f,
        p.overlay.fade(0.45 + 0.35 * f),
    );

    match art {
        Some(img) => ctx.image(img, grown, m.card_radius, 1.0),
        None => {
            let (a, b) = seed_gradient(title);
            ctx.gradient(grown, m.card_radius, a, b);
        }
    }

    // Подложка под подписями, чтобы текст читался на любом арте.
    ctx.canvas.gradient_rect(
        ctx.px(Rect::new(
            grown.x,
            grown.y + grown.h * 0.42,
            grown.w,
            grown.h * 0.58,
        )),
        p.scrim_top,
        p.scrim_bottom,
    );

    let pad = 24.0;
    let text_w = grown.w - pad * 2.0;
    ctx.text_ellipsized(
        Rect::new(grown.x + pad, grown.y + grown.h - 78.0, text_w, 34.0),
        ty.section,
        Weight::SemiBold,
        p.text,
        title,
    );
    ctx.text_ellipsized(
        Rect::new(grown.x + pad, grown.y + grown.h - 40.0, text_w, 26.0),
        ty.small,
        Weight::Regular,
        p.text_dim,
        subtitle,
    );

    if let Some(b) = &state.badge {
        badge(
            ctx,
            Rect::new(grown.x + pad, grown.y + 16.0, 0.0, 30.0),
            b,
            p.accent,
        );
    }
    if state.favorite {
        star(
            ctx,
            Rect::square(grown.x + grown.w - 46.0, grown.y + 16.0, 30.0),
            p.warning,
        );
    }
    if state.running {
        ctx.rect(
            Rect::new(
                grown.x + grown.w - 34.0,
                grown.y + grown.h - 34.0,
                14.0,
                14.0,
            ),
            7.0,
            p.success,
        );
    }
    if f > 0.01 {
        ctx.stroke(grown, m.card_radius, 3.0 * f, p.focus_ring.fade(0.85 * f));
    }
}

/// Плитка приложения в сетке: иконка (или генерируемый градиент) + подпись.
pub fn app_tile(ctx: &mut Ctx, rect: Rect, label: &str, icon: Option<&Image>, state: &TileState) {
    let p = ctx.palette();
    let m = ctx.metrics();
    let ty = ctx.typo();
    let f = state.focus.clamp(0.0, 1.0);
    let icon_rect = Rect::new(rect.x, rect.y, rect.w, rect.h - 36.0);
    let grown = icon_rect.scaled_about_center(1.0 + (m.focus_scale - 1.0) * f);

    if f > 0.01 {
        ctx.glow(grown.center(), grown.w * 0.95, p.focus_glow.fade(0.26 * f));
    }
    ctx.shadow(
        grown,
        m.tile_radius,
        12.0 + 22.0 * f,
        4.0 + 8.0 * f,
        p.overlay.fade(0.4 + 0.4 * f),
    );

    match icon {
        Some(img) => ctx.image(
            img,
            grown,
            m.tile_radius,
            if state.dimmed { 0.45 } else { 1.0 },
        ),
        None => {
            let (a, b) = seed_gradient(label);
            ctx.gradient(grown, m.tile_radius, a, b);
            let initials = label
                .chars()
                .find(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            ctx.text_center(
                grown,
                ty.hero * 0.58,
                Weight::Bold,
                Color::rgba(255, 255, 255, 235),
                &initials,
            );
        }
    }

    if state.dimmed {
        ctx.rect(grown, m.tile_radius, p.background.fade(0.55));
    }
    if let Some(b) = &state.badge {
        badge(
            ctx,
            Rect::new(grown.x + 12.0, grown.y + grown.h - 40.0, 0.0, 26.0),
            b,
            p.success,
        );
    }
    if state.favorite {
        star(
            ctx,
            Rect::square(grown.x + grown.w - 38.0, grown.y + 10.0, 24.0),
            p.warning,
        );
    }
    if f > 0.01 {
        ctx.stroke(grown, m.tile_radius, 3.0 * f, p.focus_ring.fade(0.85 * f));
    }
    if state.running {
        ctx.rect(
            Rect::new(
                rect.x + rect.w / 2.0 - 6.0,
                rect.y + rect.h - 26.0,
                12.0,
                5.0,
            ),
            3.0,
            p.success,
        );
    }

    ctx.text_ellipsized(
        Rect::new(rect.x - 14.0, rect.y + rect.h - 30.0, rect.w + 28.0, 26.0),
        ty.small,
        if f > 0.5 {
            Weight::SemiBold
        } else {
            Weight::Regular
        },
        if state.dimmed {
            p.text_faint
        } else {
            p.text_dim.mix(p.text, f)
        },
        label,
    );
}

/// Компактная «пилюля»-бейдж; ширина 0 подбирается по тексту.
pub fn badge(ctx: &mut Ctx, mut rect: Rect, text: &str, color: Color) {
    let ty = ctx.typo();
    if rect.w <= 0.0 {
        rect.w = ctx.measure(ty.small, Weight::SemiBold, text) + 24.0;
    }
    ctx.rect(rect, rect.h / 2.0, color.fade(0.22));
    ctx.stroke(rect, rect.h / 2.0, 1.0, color.fade(0.5));
    ctx.text_center(rect, ty.small, Weight::SemiBold, color, text);
}

/// Звёздочка избранного (простой полигон).
fn star(ctx: &mut Ctx, rect: Rect, color: Color) {
    ctx.text_center(rect, rect.h * 1.1, Weight::Bold, color, "★");
}

// ============================ Списки ============================

/// Строка списка: иконка, заголовок, подпись и значение справа.
#[allow(clippy::too_many_arguments)]
pub fn list_row(
    ctx: &mut Ctx,
    rect: Rect,
    title: &str,
    subtitle: Option<&str>,
    trailing: Option<&str>,
    leading: Option<&Image>,
    focus: f32,
    accent: bool,
) {
    let p = ctx.palette();
    let m = ctx.metrics();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);

    if f > 0.01 {
        ctx.shadow(
            rect,
            m.control_radius,
            20.0 * f,
            6.0 * f,
            p.overlay.fade(0.5 * f),
        );
    }
    ctx.rect(rect, m.control_radius, p.surface.mix(p.surface_focus, f));
    if f > 0.01 {
        ctx.stroke(rect, m.control_radius, 2.0 * f, p.focus_ring.fade(0.8 * f));
    }

    let pad = 22.0;
    let icon_size = rect.h.min(56.0).min(rect.h - 24.0);
    let mut text_x = rect.x + pad;

    let icon_rect = Rect::new(
        rect.x + pad,
        rect.y + (rect.h - icon_size) / 2.0,
        icon_size,
        icon_size,
    );
    if let Some(img) = leading {
        ctx.image(img, icon_rect, m.control_radius * 0.7, 1.0);
        text_x = icon_rect.x + icon_rect.w + 20.0;
    } else if accent {
        let (a, b) = seed_gradient(title);
        ctx.gradient(icon_rect, m.control_radius * 0.7, a, b);
        let initials = title
            .chars()
            .find(|c| !c.is_whitespace())
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default();
        ctx.text_center(
            icon_rect,
            ty.body,
            Weight::Bold,
            Color::rgba(255, 255, 255, 235),
            &initials,
        );
        text_x = icon_rect.x + icon_rect.w + 20.0;
    }

    let trailing_w = match trailing {
        Some(s) => ctx.measure(ty.small, Weight::Medium, s) + 20.0,
        None => 0.0,
    };
    let text_w = (rect.x + rect.w - pad - trailing_w - text_x).max(80.0);

    match subtitle {
        Some(sub) => {
            ctx.text_ellipsized(
                Rect::new(text_x, rect.y + rect.h / 2.0 - 26.0, text_w, 28.0),
                ty.body,
                Weight::Medium,
                p.text,
                title,
            );
            ctx.text_ellipsized(
                Rect::new(text_x, rect.y + rect.h / 2.0 + 2.0, text_w, 24.0),
                ty.small,
                Weight::Regular,
                p.text_dim,
                sub,
            );
        }
        None => {
            ctx.text_ellipsized(
                Rect::new(text_x, rect.y, text_w, rect.h),
                ty.body,
                Weight::Medium,
                p.text,
                title,
            );
        }
    }

    if let Some(s) = trailing {
        ctx.text_ellipsized(
            Rect::new(
                rect.x + rect.w - pad - trailing_w,
                rect.y,
                trailing_w,
                rect.h,
            ),
            ty.small,
            Weight::Medium,
            p.text_faint.mix(p.text, f * 0.5),
            s,
        );
    }
}

/// Заголовок секции (прописными) с необязательным хвостом справа.
pub fn section_title(ctx: &mut Ctx, rect: Rect, title: &str, trailing: Option<&str>) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let text = title.to_uppercase();
    let size = ty.caption;
    let baseline = rect.y + rect.h / 2.0 + size * 0.36;
    ctx.text(rect.x, baseline, size, Weight::SemiBold, p.text_dim, &text);
    if let Some(s) = trailing {
        let w = ctx.measure(ty.small, Weight::Regular, s);
        ctx.text(
            rect.x + rect.w - w,
            baseline,
            ty.small,
            Weight::Regular,
            p.text_faint,
            s,
        );
    }
}

/// Кнопка-«пилюля».
pub fn pill(ctx: &mut Ctx, rect: Rect, text: &str, active: bool, focus: f32) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);
    let r = rect.scaled_about_center(1.0 + 0.04 * f);
    let radius = r.h / 2.0;
    if f > 0.01 {
        ctx.shadow(r, radius, 16.0 * f, 4.0 * f, p.overlay.fade(0.4 * f));
    }
    if active {
        ctx.gradient(r, radius, p.accent, p.accent_soft);
        ctx.text_center(r, ty.small, Weight::SemiBold, p.on_accent, text);
    } else {
        ctx.rect(r, radius, p.surface.mix(p.surface_focus, f));
        ctx.text_center(r, ty.small, Weight::Medium, p.text_dim.mix(p.text, f), text);
    }
    if f > 0.01 {
        ctx.stroke(r, radius, 2.0 * f, p.focus_ring.fade(0.85 * f));
    }
}

/// Переключатель (вкл/выкл).
pub fn toggle(ctx: &mut Ctx, rect: Rect, on: bool, focus: f32) {
    let p = ctx.palette();
    let f = focus.clamp(0.0, 1.0);
    let h = rect.h.min(34.0);
    let w = rect.w.max(h * 1.9);
    let r = Rect::new(rect.x + rect.w - w, rect.y + (rect.h - h) / 2.0, w, h);
    ctx.rect(r, h / 2.0, if on { p.accent } else { p.surface_focus });
    if f > 0.01 {
        ctx.stroke(r, h / 2.0, 2.0 * f, p.focus_ring.fade(0.85 * f));
    }
    let knob = h - 8.0;
    let kx = if on { r.x + w - knob - 4.0 } else { r.x + 4.0 };
    let target = Rect::square(kx, r.y + 4.0, knob);
    ctx.shadow(target, knob / 2.0, 6.0, 1.0, p.overlay.fade(0.5));
    ctx.rect(target, knob / 2.0, p.text);
}

// ============================ Прогресс ============================

/// Полоса прогресса; отрицательное значение — неопределённый «бегунок».
pub fn progress(ctx: &mut Ctx, rect: Rect, value: f32, label: Option<&str>) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let h = rect.h.min(10.0);
    let bar = Rect::new(rect.x, rect.y + (rect.h - h) / 2.0, rect.w, h);

    if let Some(l) = label {
        let w = ctx.measure(ty.small, Weight::Medium, l);
        ctx.text(
            rect.x + rect.w - w,
            bar.y - 12.0,
            ty.small,
            Weight::Medium,
            p.text_dim,
            l,
        );
    }

    ctx.rect(bar, h / 2.0, p.surface_focus);
    if value < 0.0 {
        let phase = (ctx.now * 0.55).fract();
        let tri = if phase < 0.5 {
            phase * 2.0
        } else {
            2.0 - phase * 2.0
        };
        let bw = rect.w * 0.26;
        let bx = rect.x + (rect.w - bw) * tri;
        ctx.gradient(Rect::new(bx, bar.y, bw, h), h / 2.0, p.accent, p.focus_glow);
    } else {
        let v = value.clamp(0.0, 1.0);
        if v > 0.001 {
            ctx.gradient(
                Rect::new(bar.x, bar.y, bar.w * v, h),
                h / 2.0,
                p.accent,
                p.focus_glow,
            );
        }
    }
}

/// Кольцевой индикатор загрузки из точек.
pub fn spinner(ctx: &mut Ctx, center: (f32, f32), radius: f32) {
    let p = ctx.palette();
    let dots = 12usize;
    let phase = (ctx.now * 1.4) as usize % dots;
    let dot = (radius * 0.20).max(3.0);
    for i in 0..dots {
        let angle = (i as f32 / dots as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let x = center.0 + angle.cos() * radius - dot / 2.0;
        let y = center.1 + angle.sin() * radius - dot / 2.0;
        let age = (phase + dots - i) % dots;
        let alpha = 1.0 - (age as f32 / dots as f32) * 0.85;
        ctx.rect(
            Rect::square(x, y, dot),
            dot / 2.0,
            p.accent.fade(alpha.max(0.12)),
        );
    }
}

/// Полоса прокрутки.
pub fn scrollbar(ctx: &mut Ctx, rect: Rect, visible_fraction: f32, offset_fraction: f32) {
    if visible_fraction >= 0.999 {
        return;
    }
    let p = ctx.palette();
    let w = 6.0;
    ctx.rect(
        Rect::new(rect.x + rect.w - w, rect.y, w, rect.h),
        w / 2.0,
        p.surface.fade(0.7),
    );
    let thumb = (rect.h * visible_fraction.clamp(0.04, 1.0)).max(44.0);
    let y = rect.y + (rect.h - thumb) * offset_fraction.clamp(0.0, 1.0);
    ctx.rect(
        Rect::new(rect.x + rect.w - w, y, w, thumb),
        w / 2.0,
        p.text_faint,
    );
}

// ============================ Модальные окна ============================

/// Затемнение экрана под модальным окном.
pub fn dim_screen(ctx: &mut Ctx, alpha: f32) {
    let p = ctx.palette();
    ctx.canvas
        .fill_rect(ctx.px(ctx.full()), p.overlay.fade(alpha));
}

/// Тип кнопки диалога.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Основное действие.
    Primary,
    /// Второстепенное.
    Secondary,
    /// Опасное (удаление и т. п.).
    Danger,
}

/// Кнопка диалога.
#[derive(Clone, Debug)]
pub struct Button {
    /// Подпись.
    pub label: String,
    /// Тип.
    pub kind: ButtonKind,
}

impl Button {
    pub fn primary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: ButtonKind::Primary,
        }
    }
    pub fn secondary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: ButtonKind::Secondary,
        }
    }
    pub fn danger(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: ButtonKind::Danger,
        }
    }
}

/// Карточка диалога: заголовок и многострочный текст.
pub fn dialog_shell(ctx: &mut Ctx, rect: Rect, title: &str, body: Option<&str>) {
    let p = ctx.palette();
    let m = ctx.metrics();
    let ty = ctx.typo();
    ctx.shadow(rect, m.card_radius, 44.0, 10.0, p.overlay.fade(0.85));
    ctx.rect(rect, m.card_radius, p.surface_focus);
    ctx.stroke(rect, m.card_radius, 1.0, p.border);

    let pad = 34.0;
    ctx.text_ellipsized(
        Rect::new(rect.x + pad, rect.y + 26.0, rect.w - pad * 2.0, 40.0),
        ty.section,
        Weight::SemiBold,
        p.text,
        title,
    );
    if let Some(b) = body {
        wrap_text(
            ctx,
            Rect::new(
                rect.x + pad,
                rect.y + 84.0,
                rect.w - pad * 2.0,
                rect.h - 120.0,
            ),
            b,
            ty.body,
            p.text_dim,
            ty.body * 1.45,
        );
    }
}

/// Кнопки диалога в ряд; возвращает их прямоугольники.
pub fn dialog_buttons(ctx: &mut Ctx, rect: Rect, buttons: &[Button], focus: usize) -> Vec<Rect> {
    let p = ctx.palette();
    let ty = ctx.typo();
    let mut out = Vec::with_capacity(buttons.len());
    if buttons.is_empty() {
        return out;
    }
    let gap = 16.0;
    let bh = 58.0;
    let n = buttons.len() as f32;
    let bw = ((rect.w - 68.0 - gap * (n - 1.0)) / n).min(340.0);
    let total = bw * n + gap * (n - 1.0);
    let mut x = rect.x + (rect.w - total) / 2.0;
    let y = rect.y + rect.h - bh - 26.0;

    for (i, b) in buttons.iter().enumerate() {
        let r = Rect::new(x, y, bw, bh);
        out.push(r);
        let focused = i == focus;
        let rr = r.scaled_about_center(if focused { 1.04 } else { 1.0 });
        let radius = 16.0;
        if focused {
            ctx.shadow(rr, radius, 20.0, 5.0, p.overlay.fade(0.5));
        }
        match b.kind {
            ButtonKind::Primary => {
                ctx.gradient(rr, radius, p.accent, p.accent_soft);
                ctx.text_center(
                    rr,
                    ty.body,
                    Weight::SemiBold,
                    if focused {
                        p.on_accent
                    } else {
                        p.on_accent.fade(0.85)
                    },
                    &b.label,
                );
            }
            ButtonKind::Danger => {
                ctx.rect(rr, radius, p.danger.fade(if focused { 0.92 } else { 0.22 }));
                ctx.text_center(
                    rr,
                    ty.body,
                    Weight::SemiBold,
                    if focused { p.on_accent } else { p.danger },
                    &b.label,
                );
            }
            ButtonKind::Secondary => {
                ctx.rect(
                    rr,
                    radius,
                    p.surface
                        .mix(p.surface_focus, if focused { 1.0 } else { 0.4 }),
                );
                ctx.text_center(
                    rr,
                    ty.body,
                    Weight::Medium,
                    if focused { p.text } else { p.text_dim },
                    &b.label,
                );
            }
        }
        if focused {
            ctx.stroke(rr, radius, 2.0, p.focus_ring.fade(0.9));
        }
        x += bw + gap;
    }
    out
}

/// Тип всплывающего уведомления.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    /// Информация.
    Info,
    /// Успех.
    Ok,
    /// Предупреждение.
    Warn,
    /// Ошибка.
    Error,
}

/// Всплывающее уведомление внизу экрана.
pub fn toast(ctx: &mut Ctx, message: &str, kind: ToastKind, alpha: f32) {
    if alpha <= 0.01 {
        return;
    }
    let p = ctx.palette();
    let ty = ctx.typo();
    let color = match kind {
        ToastKind::Info => p.accent,
        ToastKind::Ok => p.success,
        ToastKind::Warn => p.warning,
        ToastKind::Error => p.danger,
    };
    let w = ctx.width();
    let h = ctx.height();
    let bw = (ctx.measure(ty.body, Weight::Medium, message) + 96.0).min(w - 240.0);
    let bh = 66.0;
    let r = Rect::new((w - bw) / 2.0, h - 158.0, bw, bh);

    ctx.shadow(r, bh / 2.0, 26.0, 6.0, p.overlay.fade(0.7 * alpha));
    ctx.rect(r, bh / 2.0, p.surface_focus.fade(alpha));
    ctx.stroke(r, bh / 2.0, 1.0, p.border.fade(alpha));
    ctx.rect(
        Rect::new(r.x + 16.0, r.y + 16.0, 6.0, bh - 32.0),
        3.0,
        color.fade(alpha),
    );
    ctx.text_ellipsized(
        Rect::new(r.x + 38.0, r.y, r.w - 56.0, bh),
        ty.body,
        Weight::Medium,
        p.text.fade(alpha),
        message,
    );
}

// ============================ Клавиатура ============================

/// Стереть символ.
pub const KEY_BACK: &str = "⌫";
/// Пробел.
pub const KEY_SPACE: &str = "пробел";
/// Подтвердить ввод.
pub const KEY_DONE: &str = "Готово";
/// Очистить поле.
pub const KEY_CLEAR: &str = "Очистить";
/// Сменить раскладку.
pub const KEY_LAYOUT: &str = "RU/EN";

/// Экранная клавиатура для пульта.
#[derive(Clone, Debug)]
pub struct Keyboard {
    /// Раскладка по рядам.
    pub rows: Vec<Vec<String>>,
    /// Активный ряд.
    pub row: usize,
    /// Активная колонка.
    pub col: usize,
}

impl Keyboard {
    /// Русская раскладка.
    pub fn russian() -> Self {
        Self {
            rows: vec![
                chars_row("1234567890"),
                chars_row("йцукенгшщзхъ"),
                chars_row("фывапролджэё"),
                chars_row("ячсмитьбю"),
                vec![
                    "-".into(),
                    ".".into(),
                    ",".into(),
                    ":".into(),
                    "/".into(),
                    KEY_SPACE.into(),
                    KEY_LAYOUT.into(),
                    KEY_BACK.into(),
                ],
                vec![KEY_CLEAR.into(), KEY_DONE.into()],
            ],
            row: 1,
            col: 0,
        }
    }

    /// Латинская раскладка.
    pub fn english() -> Self {
        Self {
            rows: vec![
                chars_row("1234567890"),
                chars_row("qwertyuiop"),
                chars_row("asdfghjkl"),
                chars_row("zxcvbnm"),
                vec![
                    "-".into(),
                    ".".into(),
                    "_".into(),
                    ":".into(),
                    "/".into(),
                    KEY_SPACE.into(),
                    KEY_LAYOUT.into(),
                    KEY_BACK.into(),
                ],
                vec![KEY_CLEAR.into(), KEY_DONE.into()],
            ],
            row: 1,
            col: 0,
        }
    }

    /// Раскладка для ввода URL.
    pub fn url() -> Self {
        Self {
            rows: vec![
                chars_row("1234567890"),
                chars_row("qwertyuiop"),
                chars_row("asdfghjkl"),
                chars_row("zxcvbnm"),
                vec![
                    ".".into(),
                    "-".into(),
                    "_".into(),
                    "/".into(),
                    ":".into(),
                    "?".into(),
                    "&".into(),
                    KEY_BACK.into(),
                ],
                vec![KEY_CLEAR.into(), KEY_SPACE.into(), KEY_DONE.into()],
            ],
            row: 1,
            col: 0,
        }
    }

    /// Клавиша под курсором.
    pub fn key(&self) -> &str {
        self.rows
            .get(self.row)
            .and_then(|r| r.get(self.col.min(r.len().saturating_sub(1))))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Перемещение курсора (циклическое).
    pub fn move_focus(&mut self, dx: i32, dy: i32) {
        if dy != 0 {
            let nrows = self.rows.len() as i32;
            self.row = (self.row as i32 + dy).rem_euclid(nrows) as usize;
        }
        let len = self.rows[self.row].len().max(1) as i32;
        self.col = (self.col as i32 + dx).rem_euclid(len) as usize;
    }

    /// Число рядов.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Пуста ли раскладка.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

fn chars_row(s: &str) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

/// Рисует клавиатуру внутри `rect`.
pub fn keyboard(ctx: &mut Ctx, rect: Rect, kb: &Keyboard) {
    let p = ctx.palette();
    let ty = ctx.typo();
    ctx.shadow(rect, 24.0, 30.0, 8.0, p.overlay.fade(0.5));
    ctx.rect(rect, 24.0, p.surface.fade(0.96));
    ctx.stroke(rect, 24.0, 1.0, p.border);

    let pad = 16.0;
    let gap = 9.0;
    let rows = kb.len().max(1) as f32;
    let kh = (rect.h - pad * 2.0 - gap * (rows - 1.0)) / rows;

    for (ri, row) in kb.rows.iter().enumerate() {
        let y = rect.y + pad + ri as f32 * (kh + gap);
        let n = row.len().max(1) as f32;
        let kw = (rect.w - pad * 2.0 - gap * (n - 1.0)) / n;
        for (ci, key) in row.iter().enumerate() {
            let x = rect.x + pad + ci as f32 * (kw + gap);
            let r = Rect::new(x, y, kw, kh);
            let focused = ri == kb.row && ci == kb.col;
            let rr = r.scaled_about_center(if focused { 1.08 } else { 1.0 });
            let special = matches!(
                key.as_str(),
                KEY_BACK | KEY_DONE | KEY_CLEAR | KEY_LAYOUT | KEY_SPACE
            );
            let size = if special || key.chars().count() > 1 {
                ty.small
            } else {
                ty.body
            };
            if focused {
                ctx.shadow(rr, 10.0, 14.0, 3.0, p.overlay.fade(0.45));
                ctx.gradient(rr, 10.0, p.accent, p.accent_soft);
                ctx.text_center(rr, size, Weight::SemiBold, p.on_accent, key);
            } else {
                ctx.rect(rr, 10.0, if special { p.surface } else { p.surface_focus });
                ctx.text_center(
                    rr,
                    size,
                    Weight::Medium,
                    if special { p.text_dim } else { p.text },
                    key,
                );
            }
        }
    }
}

/// Поле ввода текста с мигающим курсором.
pub fn text_field(ctx: &mut Ctx, rect: Rect, value: &str, hint: &str, focus: f32, caret: bool) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);
    ctx.rect(rect, 16.0, p.surface.mix(p.surface_focus, f));
    if f > 0.01 {
        ctx.stroke(rect, 16.0, 2.0 * f, p.accent.fade(0.5 + 0.5 * f));
    }
    let pad = 24.0;
    let inner = Rect::new(rect.x + pad, rect.y, rect.w - pad * 2.0, rect.h);
    let baseline = inner.y + inner.h / 2.0 + ty.body * 0.36;

    if value.is_empty() {
        ctx.text_ellipsized(inner, ty.body, Weight::Regular, p.text_faint, hint);
        return;
    }
    let size = ty.body;
    let weight = Weight::Medium;
    let shortened = {
        let Ctx {
            fonts,
            cache,
            scale,
            ..
        } = ctx;
        crate::gfx::ellipsize(fonts, cache, size * *scale, weight, value, inner.w * *scale)
    };
    let drawn = ctx.text(inner.x, baseline, size, weight, p.text, &shortened);
    if caret && ((ctx.now * 1.5).fract() > 0.45) {
        ctx.rect(
            Rect::new(inner.x + drawn + 5.0, inner.y + 16.0, 3.0, rect.h - 32.0),
            1.5,
            p.accent,
        );
    }
}

// ============================ QR, текст, подсказки ============================

/// QR-код, вписанный в прямоугольник (по центру, со светлым полем).
pub fn qr(ctx: &mut Ctx, rect: Rect, matrix: &QrMatrix) {
    let p = ctx.palette();
    let total = matrix.total_size().max(1) as f32;
    let size = rect.w.min(rect.h);
    let cell = size / total;
    let ox = rect.x + (rect.w - size) / 2.0;
    let oy = rect.y + (rect.h - size) / 2.0;

    let board = Rect::new(ox - 12.0, oy - 12.0, size + 24.0, size + 24.0);
    ctx.shadow(board, 18.0, 22.0, 6.0, p.overlay.fade(0.4));
    ctx.rect(board, 18.0, Color::rgb(250, 250, 252));

    let fg = Color::rgb(16, 18, 24);
    for y in 0..matrix.total_size() {
        for x in 0..matrix.total_size() {
            if !matrix.get_padded(x, y) {
                continue;
            }
            let r = Rect::new(ox + x as f32 * cell, oy + y as f32 * cell, cell, cell);
            // Небольшое перекрытие, чтобы между модулями не было щелей.
            ctx.canvas
                .fill_rect(ctx.px(Rect::new(r.x, r.y, r.w + 0.6, r.h + 0.6)), fg);
        }
    }
}

/// Многострочный текст с переносом по словам; возвращает занятую высоту.
pub fn wrap_text(
    ctx: &mut Ctx,
    rect: Rect,
    text: &str,
    size: f32,
    color: Color,
    line_height: f32,
) -> f32 {
    let mut y = rect.y;
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && ctx.measure(size, Weight::Regular, &candidate) > rect.w {
                ctx.text(rect.x, y + size * 0.8, size, Weight::Regular, color, &line);
                y += line_height;
                line = word.to_string();
                if y + line_height > rect.y + rect.h {
                    return y - rect.y;
                }
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            ctx.text(rect.x, y + size * 0.8, size, Weight::Regular, color, &line);
            y += line_height;
        }
        if para.is_empty() {
            y += line_height * 0.45;
        }
    }
    y - rect.y
}

/// Пустое состояние раздела.
pub fn empty_state(ctx: &mut Ctx, rect: Rect, glyph: &str, title: &str, hint: &str) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let cy = rect.y + rect.h * 0.42;
    ctx.text_center(
        Rect::new(rect.x, cy - 96.0, rect.w, 120.0),
        ty.hero * 0.85,
        Weight::Bold,
        p.text_faint.fade(0.55),
        glyph,
    );
    ctx.text_center(
        Rect::new(rect.x, cy + 20.0, rect.w, 44.0),
        ty.section,
        Weight::SemiBold,
        p.text_dim,
        title,
    );
    wrap_text(
        ctx,
        Rect::new(rect.x + rect.w * 0.2, cy + 84.0, rect.w * 0.6, 160.0),
        hint,
        ty.body,
        p.text_faint,
        ty.body * 1.5,
    );
}

/// Подсказки по кнопкам пульта внизу экрана.
pub fn key_hints(ctx: &mut Ctx, hints: &[(&str, &str)]) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let m = ctx.metrics().margin;
    let y = ctx.height() - 56.0;
    let mut x = m;
    for (key, label) in hints {
        let kw = ctx.measure(ty.small, Weight::Bold, key) + 22.0;
        let kr = Rect::new(x, y, kw, 30.0);
        ctx.rect(kr, 8.0, p.surface_focus);
        ctx.stroke(kr, 8.0, 1.0, p.border);
        ctx.text_center(kr, ty.small, Weight::Bold, p.text_dim, key);
        x += kw + 10.0;
        let lw = ctx.measure(ty.small, Weight::Regular, label);
        ctx.text(
            x,
            y + 15.0 + ty.small * 0.36,
            ty.small,
            Weight::Regular,
            p.text_faint,
            label,
        );
        x += lw + 30.0;
    }
}

/// Карточка приложения в магазине: иконка, название, версия, кнопка.
pub fn store_card(
    ctx: &mut Ctx,
    rect: Rect,
    title: &str,
    subtitle: &str,
    action: &str,
    icon: Option<&Image>,
    focus: f32,
) {
    let p = ctx.palette();
    let m = ctx.metrics();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);
    let r = rect.scaled_about_center(1.0 + 0.03 * f);

    if f > 0.01 {
        ctx.glow(r.center(), r.w * 0.6, p.focus_glow.fade(0.18 * f));
        ctx.shadow(r, m.card_radius, 26.0 * f, 8.0 * f, p.overlay.fade(0.5 * f));
    }
    ctx.rect(r, m.card_radius, p.surface.mix(p.surface_focus, f));
    ctx.stroke(
        r,
        m.card_radius,
        1.0 + f,
        p.border.mix(p.focus_ring, f * 0.5),
    );

    let pad = 22.0;
    let icon_size = 84.0;
    let icon_rect = Rect::new(r.x + pad, r.y + pad, icon_size, icon_size);
    match icon {
        Some(img) => ctx.image(img, icon_rect, m.control_radius, 1.0),
        None => {
            let (a, b) = seed_gradient(title);
            ctx.gradient(icon_rect, m.control_radius, a, b);
            let initials = title
                .chars()
                .find(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            ctx.text_center(
                icon_rect,
                ty.section,
                Weight::Bold,
                Color::rgba(255, 255, 255, 235),
                &initials,
            );
        }
    }

    let text_x = icon_rect.x + icon_size + 20.0;
    let text_w = r.x + r.w - pad - text_x;
    ctx.text_ellipsized(
        Rect::new(text_x, r.y + pad + 2.0, text_w, 32.0),
        ty.body,
        Weight::SemiBold,
        p.text,
        title,
    );
    ctx.text_ellipsized(
        Rect::new(text_x, r.y + pad + 34.0, text_w, 26.0),
        ty.small,
        Weight::Regular,
        p.text_dim,
        subtitle,
    );

    let bw = (ctx.measure(ty.small, Weight::SemiBold, action) + 40.0).min(r.w - pad * 2.0);
    pill(
        ctx,
        Rect::new(r.x + r.w - pad - bw, r.y + r.h - 62.0, bw, 44.0),
        action,
        true,
        f,
    );
}

// ============================ Детерминированные цвета ============================

fn hash_seed(seed: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Детерминированная пара цветов для приложения без иконки.
pub fn seed_gradient(seed: &str) -> (Color, Color) {
    let h = hash_seed(seed);
    let hue = (h % 360) as f32;
    let hue2 = (hue + 32.0 + ((h >> 16) % 48) as f32) % 360.0;
    (hsv(hue, 0.58, 0.74), hsv(hue2, 0.70, 0.44))
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
    let c = v * s;
    let hh = (h % 360.0) / 60.0;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hh as usize {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Color::rgb(
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

/// Пункт верхней навигации: пиктограмма + подпись.
pub fn nav_item(ctx: &mut Ctx, rect: Rect, glyph: &str, label: &str, active: bool, focus: f32) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);
    let r = rect.scaled_about_center(1.0 + 0.05 * f);
    let radius = 16.0;

    if f > 0.01 {
        ctx.shadow(r, radius, 20.0 * f, 5.0 * f, p.overlay.fade(0.45 * f));
    }
    if active {
        ctx.gradient(r, radius, p.accent.fade(0.28), p.accent_soft.fade(0.16));
    } else {
        ctx.rect(r, radius, p.surface.fade(0.55 + 0.45 * f));
    }
    if f > 0.01 {
        ctx.stroke(r, radius, 2.0 * f, p.focus_ring.fade(0.85 * f));
    }

    let glyph_size = ty.body;
    let label_size = ty.body;
    let total = ctx.measure(glyph_size, Weight::Bold, glyph)
        + 12.0
        + ctx.measure(
            label_size,
            if active {
                Weight::SemiBold
            } else {
                Weight::Medium
            },
            label,
        );
    let mut x = r.x + (r.w - total) / 2.0;
    let baseline = r.y + r.h / 2.0 + glyph_size * 0.36;
    let glyph_color = if active {
        p.accent.mix(p.text, 0.35)
    } else {
        p.text_faint.mix(p.text_dim, f)
    };
    let drawn = ctx.text(x, baseline, glyph_size, Weight::Bold, glyph_color, glyph);
    x += drawn + 12.0;
    let text_color = if active {
        p.text
    } else {
        p.text_dim.mix(p.text, f)
    };
    ctx.text(
        x,
        baseline,
        label_size,
        if active {
            Weight::SemiBold
        } else {
            Weight::Medium
        },
        text_color,
        label,
    );

    if active {
        ctx.rect(
            Rect::new(r.x + r.w * 0.22, r.y + r.h - 6.0, r.w * 0.56, 5.0),
            3.0,
            p.accent,
        );
    }
}

/// Строка «ключ — значение» (экран «О системе», детали приложения).
pub fn info_row(ctx: &mut Ctx, rect: Rect, key: &str, value: &str, focus: f32) {
    let p = ctx.palette();
    let ty = ctx.typo();
    let f = focus.clamp(0.0, 1.0);
    if f > 0.01 {
        ctx.rect(rect, 12.0, p.surface.mix(p.surface_focus, f));
        ctx.stroke(rect, 12.0, 2.0 * f, p.focus_ring.fade(0.8 * f));
    } else {
        ctx.rect(rect, 12.0, p.surface.fade(0.6));
    }
    let pad = 24.0;
    let key_w = rect.w * 0.38;
    ctx.text_ellipsized(
        Rect::new(rect.x + pad, rect.y, key_w, rect.h),
        ty.body,
        Weight::Regular,
        p.text_dim,
        key,
    );
    ctx.text_ellipsized(
        Rect::new(
            rect.x + pad + key_w,
            rect.y,
            rect.w - key_w - pad * 2.0,
            rect.h,
        ),
        ty.body,
        Weight::Medium,
        p.text.mix(p.accent, f * 0.15),
        value,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{Canvas, FontSet, TextCache};
    use nova_core::settings::Theme as ThemeChoice;

    fn render<F: FnOnce(&mut Ctx)>(width: u32, height: u32, draw: F) -> Canvas {
        let mut canvas = Canvas::new(width, height);
        let fonts = FontSet::embedded();
        let mut cache = TextCache::new();
        let theme = Theme::for_choice(ThemeChoice::Nova);
        {
            let mut ctx = Ctx::new(&mut canvas, &fonts, &mut cache, theme, 1.25);
            draw(&mut ctx);
        }
        canvas
    }

    #[test]
    fn ctx_scale_follows_resolution() {
        render(1920, 1080, |ctx| {
            assert!((ctx.scale - 1.0).abs() < 1e-6);
            assert_eq!(ctx.width() as u32, 1920);
            assert_eq!(ctx.height() as u32, 1080);
        });
        render(1280, 720, |ctx| {
            assert!((ctx.scale - 2.0 / 3.0).abs() < 1e-4);
            assert_eq!(ctx.width().round() as u32, 1920);
        });
    }

    #[test]
    fn widgets_paint_pixels() {
        let canvas = render(640, 360, |ctx| {
            background(ctx, None);
            top_bar(ctx, "Приложения", "21:40", Some("12 установленных"));
            app_tile(
                ctx,
                Rect::new(96.0, 300.0, 168.0, 204.0),
                "Luma",
                None,
                &TileState::focused(),
            );
            list_row(
                ctx,
                Rect::new(400.0, 300.0, 900.0, 84.0),
                "Репозиторий",
                Some("webosbrew"),
                Some("обновить"),
                None,
                0.5,
                true,
            );
            progress(ctx, Rect::new(400.0, 460.0, 900.0, 10.0), 0.42, Some("42%"));
            spinner(ctx, (1300.0, 700.0), 26.0);
            toast(ctx, "Установка завершена", ToastKind::Ok, 1.0);
            pill(
                ctx,
                Rect::new(96.0, 900.0, 220.0, 48.0),
                "Установить",
                true,
                1.0,
            );
        });
        let painted = canvas
            .pixels
            .chunks_exact(4)
            .filter(|px| u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2]) > 60)
            .count();
        assert!(painted > 5000, "слишком мало пикселей: {painted}");
    }

    #[test]
    fn keyboard_navigation_is_cyclic() {
        let mut kb = Keyboard::russian();
        assert_eq!(kb.key(), "й");
        kb.move_focus(-1, 0);
        assert_eq!(kb.key(), "ъ");
        kb.move_focus(1, 0);
        assert_eq!(kb.key(), "й");
        kb.move_focus(0, -1);
        assert_eq!(kb.key(), "1");
        for _ in 0..30 {
            kb.move_focus(3, 1);
        }
        assert!(!kb.key().is_empty());
        let url = Keyboard::url();
        assert!(url.rows.iter().flatten().any(|k| k == "/"));
        assert!(!Keyboard::english().is_empty());
    }

    #[test]
    fn seed_gradient_is_stable_and_colored() {
        let (a1, b1) = seed_gradient("Luma");
        let (a2, _) = seed_gradient("Luma");
        assert_eq!(a1, a2);
        assert_ne!(a1, b1);
        assert!(u32::from(a1.r) + u32::from(a1.g) + u32::from(a1.b) > 60);
        assert_ne!(seed_gradient("Luma").0, seed_gradient("HBChannel").0);
        // Кэш глифов действительно наполняется при отрисовке виджетов.
        assert!(!canvas_cache_used());
    }

    fn canvas_cache_used() -> bool {
        let mut canvas = Canvas::new(320, 120);
        let fonts = FontSet::embedded();
        let mut cache = TextCache::new();
        let theme = Theme::for_choice(ThemeChoice::Nova);
        {
            let mut ctx = Ctx::new(&mut canvas, &fonts, &mut cache, theme, 0.0);
            ctx.text(
                10.0,
                60.0,
                28.0,
                Weight::Bold,
                Color::rgb(255, 255, 255),
                "Установка",
            );
        }
        cache.glyph_count() == 0
    }

    #[test]
    fn wrap_text_respects_width() {
        render(960, 540, |ctx| {
            let used = wrap_text(
                ctx,
                Rect::new(100.0, 100.0, 600.0, 400.0),
                "Длинный текст, который должен переноситься по словам и занимать несколько строк",
                28.0,
                ctx.palette().text,
                40.0,
            );
            assert!(used > 40.0 && used <= 400.0, "высота {used}");
        });
    }
}
