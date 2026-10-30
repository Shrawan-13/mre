//! Lua tables to declarative views.
//!
//! This is the whole surface area between an application written in Lua and the
//! engine. It is intentionally forgiving: unknown keys are ignored and malformed
//! values fall back to defaults, because a typo in an app should produce a wrong
//! pixel, not a crash.

use mlua::{Function, Table, Value};
use mre_core::Rgba8;
use mre_layout::{Align, Dim, Edges, Justify, Track};
use mre_text::{TextAlign, TextStyle, Wrap};
use mre_widgets::view::IconKind;
use mre_widgets::{Theme, View};
use std::collections::HashMap;

/// Event handlers collected during conversion, keyed by the node's key.
#[derive(Debug, Default)]
pub struct Handlers {
    /// Click handlers.
    pub click: HashMap<String, Function>,
    /// Scroll handlers, called with `(dx, dy)` in logical pixels. Positive `dy`
    /// means "scroll down", that is, the content moves up.
    pub scroll: HashMap<String, Function>,
    /// Drag handlers, called with `(x, y)` relative to the node's top-left while
    /// the pointer is down.
    pub drag: HashMap<String, Function>,
    /// Text change handlers, called with the new value.
    pub change: HashMap<String, Function>,
    /// Submit handlers, called with the current value when Enter is pressed.
    pub submit: HashMap<String, Function>,
}

impl Handlers {
    /// Total number of handlers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.click.len()
            + self.scroll.len()
            + self.drag.len()
            + self.change.len()
            + self.submit.len()
    }

    /// True when no handler was declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn ensure_key(key: &mut Option<String>, counter: &mut u32) -> String {
    key.get_or_insert_with(|| {
        *counter += 1;
        format!("@auto{counter}")
    })
    .clone()
}

fn get<T: mlua::FromLua>(table: &Table, key: &str) -> Option<T> {
    table.get::<Option<T>>(key).ok().flatten()
}

fn number(table: &Table, key: &str) -> Option<f32> {
    get::<f64>(table, key).map(|value| value as f32)
}

fn string(table: &Table, key: &str) -> Option<String> {
    match table.get::<Value>(key).ok()? {
        Value::String(value) => Some(value.to_string_lossy()),
        Value::Integer(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn colour_value(value: &Value, theme: &Theme) -> Option<Rgba8> {
    match value {
        Value::String(text) => {
            let text = text.to_string_lossy();
            let text = text.trim();
            if let Some(hex) = text.strip_prefix('#') {
                return Rgba8::parse_hex(&format!("#{hex}"));
            }
            let colour = match text {
                "transparent" => Rgba8::TRANSPARENT,
                "background" => theme.background,
                "surface" => theme.surface,
                "surface_alt" => theme.surface_alt,
                "border" => theme.border,
                "text" => theme.text,
                "text_muted" => theme.text_muted,
                "accent" => theme.accent,
                "on_accent" => theme.on_accent,
                _ => return None,
            };
            Some(colour)
        }
        Value::Table(table) => {
            let channel = |key: &str, default: u8| -> u8 {
                table
                    .get::<Option<f64>>(key)
                    .ok()
                    .flatten()
                    .map_or(default, |value| value.clamp(0.0, 255.0) as u8)
            };
            Some(Rgba8::from_rgba(
                channel("r", 0),
                channel("g", 0),
                channel("b", 0),
                channel("a", 255),
            ))
        }
        _ => None,
    }
}

fn colour(table: &Table, key: &str, theme: &Theme) -> Option<Rgba8> {
    table
        .get::<Value>(key)
        .ok()
        .and_then(|value| colour_value(&value, theme))
}

fn dimension(value: &Value) -> Option<Dim> {
    match value {
        Value::Integer(value) => Some(Dim::Px(*value as f32)),
        Value::Number(value) => Some(Dim::Px(*value as f32)),
        Value::String(text) => {
            let text = text.to_string_lossy();
            let text = text.trim();
            if let Some(percent) = text.strip_suffix('%') {
                percent.parse::<f32>().ok().map(Dim::Percent)
            } else if let Some(pixels) = text.strip_suffix("px") {
                pixels.parse::<f32>().ok().map(Dim::Px)
            } else {
                text.parse::<f32>().ok().map(Dim::Px)
            }
        }
        _ => None,
    }
}

fn dim(table: &Table, key: &str) -> Option<Dim> {
    table
        .get::<Value>(key)
        .ok()
        .and_then(|value| dimension(&value))
}

fn edges(value: &Value) -> Option<Edges> {
    match value {
        Value::Integer(value) => Some(Edges::all(*value as f32)),
        Value::Number(value) => Some(Edges::all(*value as f32)),
        Value::Table(table) => {
            let all = table
                .get::<Option<f64>>("all")
                .ok()
                .flatten()
                .map_or(0.0, |value| value as f32);
            let side = |key: &str| -> f32 {
                table
                    .get::<Option<f64>>(key)
                    .ok()
                    .flatten()
                    .map_or(all, |value| value as f32)
            };
            Some(Edges::new(
                side("top"),
                side("right"),
                side("bottom"),
                side("left"),
            ))
        }
        _ => None,
    }
}

fn align_of(text: &str) -> Option<Align> {
    match text {
        "start" | "left" | "top" => Some(Align::Start),
        "center" | "centre" => Some(Align::Center),
        "end" | "right" | "bottom" => Some(Align::End),
        "stretch" => Some(Align::Stretch),
        _ => None,
    }
}

fn justify_of(text: &str) -> Option<Justify> {
    match text {
        "start" | "left" => Some(Justify::Start),
        "center" | "centre" => Some(Justify::Center),
        "end" | "right" => Some(Justify::End),
        "between" | "space-between" => Some(Justify::SpaceBetween),
        "around" | "space-around" => Some(Justify::SpaceAround),
        "evenly" | "space-evenly" => Some(Justify::SpaceEvenly),
        _ => None,
    }
}

fn text_align_of(text: &str) -> Option<TextAlign> {
    match text {
        "start" | "left" => Some(TextAlign::Start),
        "center" | "centre" => Some(TextAlign::Center),
        "end" | "right" => Some(TextAlign::End),
        _ => None,
    }
}

fn track(value: &Value) -> Option<Track> {
    match value {
        Value::Integer(value) => Some(Track::Px(*value as f32)),
        Value::Number(value) => Some(Track::Px(*value as f32)),
        Value::String(text) => {
            let text = text.to_string_lossy();
            let text = text.trim();
            if let Some(fraction) = text.strip_suffix("fr") {
                fraction.parse::<f32>().ok().map(Track::Fr)
            } else if text == "auto" {
                Some(Track::Auto)
            } else if let Some(pixels) = text.strip_suffix("px") {
                pixels.parse::<f32>().ok().map(Track::Px)
            } else {
                text.parse::<f32>().ok().map(Track::Px)
            }
        }
        _ => None,
    }
}

fn text_style(table: &Table, theme: &Theme) -> TextStyle {
    let mut style = TextStyle::new(number(table, "size").unwrap_or(theme.font_size));
    if let Some(weight) = number(table, "weight") {
        style.weight = weight.clamp(100.0, 900.0) as u16;
    }
    if let Some(italic) = get::<bool>(table, "italic") {
        style.italic = italic;
    }
    if let Some(line_height) = number(table, "line_height") {
        style.line_height = Some(line_height);
    }
    if let Some(spacing) = number(table, "letter_spacing") {
        style.letter_spacing = spacing;
    }
    if let Some(family) = string(table, "family") {
        style.family = Some(family);
    }
    if let Some(align) = string(table, "text_align").and_then(|value| text_align_of(&value)) {
        style.align = align;
    }
    if let Some(wrap) = get::<bool>(table, "nowrap") {
        if wrap {
            style.wrap = Wrap::None;
        }
    }
    style
}

/// Converts one vnode table, recursively.
///
/// # Errors
///
/// Returns an error only when Lua itself fails, for example while reading a
/// proxy table. Malformed fields are ignored.
pub fn convert(
    table: &Table,
    theme: &Theme,
    handlers: &mut Handlers,
    counter: &mut u32,
) -> mlua::Result<View> {
    let kind = string(table, "__mre").unwrap_or_else(|| "column".to_string());

    let mut view = match kind.as_str() {
        "text" => {
            let text = string(table, "text").unwrap_or_default();
            let style = text_style(table, theme);
            let colour = colour(table, "color", theme).unwrap_or(theme.text);
            View::text(text, style).color(colour)
        }
        "button" => {
            let label = string(table, "text").or_else(|| {
                // `mre.button { "Refresh" }` puts the label in the child array.
                table
                    .get::<Option<Table>>("children")
                    .ok()
                    .flatten()
                    .and_then(|children| children.get::<Option<String>>(1).ok().flatten())
            });
            View::button(label.unwrap_or_default())
        }
        "spacer" => View::spacer(number(table, "size").unwrap_or(0.0)),
        "text_input" => {
            let value = string(table, "value").unwrap_or_default();
            let placeholder = string(table, "placeholder").unwrap_or_default();
            let size = number(table, "size").unwrap_or(14.0);
            let password = get::<bool>(table, "password").unwrap_or(false);
            let mut view = View::text_input(value, placeholder, size);
            if password {
                if let mre_widgets::ViewKind::TextInput(input) = &mut view.kind {
                    input.password = true;
                }
            }
            view
        }
        "icon" => {
            let kind = string(table, "kind")
                .and_then(|name| IconKind::parse(&name))
                .unwrap_or(IconKind::Check);
            let size = number(table, "size").unwrap_or(16.0);
            View::icon(kind, size)
        }
        "list" => {
            let count = number(table, "count").unwrap_or(0.0).max(0.0) as usize;
            let row_height = number(table, "row_height").unwrap_or(24.0).max(1.0);
            let offset = number(table, "offset").unwrap_or(0.0).max(0.0);
            let viewport = number(table, "height").unwrap_or(400.0).max(row_height);

            let total = count as f32 * row_height;
            let mut view = View::stack().height(Dim::Px(total)).clip(true);

            let first = (offset / row_height).floor().max(0.0) as usize;
            let visible = (viewport / row_height).ceil() as usize + 3;
            let start = first.saturating_sub(1).min(count);
            let end = (first + visible).min(count);

            if let Ok(Some(render)) = table.get::<Option<Function>>("render") {
                for index in start..end {
                    let Ok(Some(child)) = render.call::<Option<Table>>(index as i64 + 1) else {
                        continue;
                    };
                    let mut child = convert(&child, theme, handlers, counter)?;
                    if child.style.height == Dim::Auto {
                        child.style.height = Dim::Px(row_height);
                    }
                    child.style.position = mre_layout::Position::Absolute;
                    child.style.inset = [
                        Some(index as f32 * row_height - offset),
                        Some(0.0),
                        None,
                        Some(0.0),
                    ];
                    view.children.push(child);
                }
            }
            return Ok(view);
        }
        "grid" => {
            let columns = table
                .get::<Option<Table>>("columns")
                .ok()
                .flatten()
                .map(|columns| {
                    columns
                        .sequence_values::<Value>()
                        .filter_map(Result::ok)
                        .filter_map(|value| track(&value))
                        .collect()
                })
                .unwrap_or_default();
            View::grid(columns)
        }
        "stack" | "layer" => View::stack(),
        "row" => View::row(),
        "scroll" => View::column().clip(true),
        // `scroll`, `image` and `canvas` are containers until their dedicated
        // implementations land; they must still lay out their children.
        _ => View::column(),
    };

    // Layout properties.
    if let Ok(value) = table.get::<Value>("padding") {
        if let Some(edges) = edges(&value) {
            view.style.padding = edges;
        }
    }
    if let Ok(value) = table.get::<Value>("margin") {
        if let Some(edges) = edges(&value) {
            view.style.margin = edges;
        }
    }
    if let Some(gap) = number(table, "gap") {
        view.style.gap = gap.max(0.0);
    }
    if let Some(width) = dim(table, "width") {
        view.style.width = width;
    }
    if kind != "list" {
        if let Some(height) = dim(table, "height") {
            view.style.height = height;
        }
    }
    if let Some(width) = dim(table, "min_width") {
        view.style.min_width = width;
    }
    if let Some(width) = dim(table, "max_width") {
        view.style.max_width = width;
    }
    if let Some(height) = dim(table, "min_height") {
        view.style.min_height = height;
    }
    if let Some(height) = dim(table, "max_height") {
        view.style.max_height = height;
    }
    if let Some(grow) = number(table, "grow") {
        view.style.flex_grow = grow.max(0.0);
    }
    if let Some(align) = string(table, "align").and_then(|value| align_of(&value)) {
        view.style.align_items = align;
    }
    if let Some(justify) = string(table, "justify").and_then(|value| justify_of(&value)) {
        view.style.justify_content = justify;
    }
    if get::<bool>(table, "absolute").unwrap_or(false) {
        view.style.position = mre_layout::Position::Absolute;
        view.style.inset = [
            number(table, "top"),
            number(table, "right"),
            number(table, "bottom"),
            number(table, "left"),
        ];
    }

    // Paint properties.
    if let Some(background) = colour(table, "background", theme) {
        view.props.background = Some(background);
    }
    if let Some(radius) = number(table, "radius") {
        view.props.radius = radius.max(0.0);
    }
    if let Some(opacity) = number(table, "opacity") {
        view.props.opacity = opacity.clamp(0.0, 1.0);
    }
    if get::<bool>(table, "interactive").unwrap_or(false) {
        view.props.interactive = true;
    }
    if get::<bool>(table, "clip").unwrap_or(false)
        || string(table, "overflow").as_deref() == Some("hidden")
    {
        view.props.clip = true;
    }
    if kind != "text" {
        if let Some(colour) = colour(table, "color", theme) {
            view.props.color = Some(colour);
        }
    }
    if let Ok(Value::Table(border)) = table.get::<Value>("border") {
        let border_colour = colour(&border, "color", theme).unwrap_or(theme.border);
        let width = number(&border, "width").unwrap_or(1.0);
        view.props.border = Some((border_colour, width));
    } else if let Some(border_colour) = colour(table, "border", theme) {
        view.props.border = Some((border_colour, 1.0));
    }

    // Children.
    if let Ok(Some(children)) = table.get::<Option<Table>>("children") {
        for child in children.sequence_values::<Table>().filter_map(Result::ok) {
            view.children
                .push(convert(&child, theme, handlers, counter)?);
        }
    }

    // Identity.
    if let Some(key) = string(table, "key") {
        view.key = Some(key);
    }

    // Events.
    if let Ok(Some(handler)) = table.get::<Option<Function>>("on_click") {
        let key = ensure_key(&mut view.key, counter);
        view.props.interactive = true;
        handlers.click.insert(key, handler);
    }
    if let Ok(Some(handler)) = table.get::<Option<Function>>("on_scroll") {
        let key = ensure_key(&mut view.key, counter);
        handlers.scroll.insert(key, handler);
    }
    if let Ok(Some(handler)) = table.get::<Option<Function>>("on_drag") {
        let key = ensure_key(&mut view.key, counter);
        view.props.interactive = true;
        handlers.drag.insert(key, handler);
    }
    if let Ok(Some(handler)) = table.get::<Option<Function>>("on_change") {
        let key = ensure_key(&mut view.key, counter);
        handlers.change.insert(key, handler);
    }
    if let Ok(Some(handler)) = table.get::<Option<Function>>("on_submit") {
        let key = ensure_key(&mut view.key, counter);
        handlers.submit.insert(key, handler);
    }

    Ok(view)
}

/// A view plus the handlers it declared.
pub struct ConvertedView {
    /// The root view.
    pub view: View,
    /// Handlers keyed by node key.
    pub handlers: Handlers,
}

impl std::fmt::Debug for ConvertedView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConvertedView")
            .field("kind", &self.view.kind)
            .field("handlers", &self.handlers.len())
            .finish()
    }
}

/// Converts a root vnode table.
///
/// # Errors
///
/// Propagates Lua errors.
pub fn convert_root(table: &Table, theme: &Theme) -> mlua::Result<ConvertedView> {
    let mut handlers = Handlers::default();
    let mut counter = 0;
    let view = convert(table, theme, &mut handlers, &mut counter)?;
    Ok(ConvertedView { view, handlers })
}

/// Counts nodes in a converted view, for diagnostics.
#[must_use]
pub fn count(view: &View) -> usize {
    1 + view.children.iter().map(count).sum::<usize>()
}
