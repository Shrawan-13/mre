//! Lua tables to declarative views.
//!
//! This is the whole surface area between an application written in Lua and the
//! engine. It is intentionally forgiving: unknown keys are ignored and malformed
//! values fall back to defaults, because a typo in an app should produce a wrong
//! pixel, not a crash.

use mlua::{Function, Table, Value};
use mre_core::Rgba8;
use mre_layout::{Align, Dim, Direction, Display, Edges, Justify, Position, Track};
use mre_text::{TextAlign, TextStyle, Wrap};
use mre_widgets::ui::AnimProp;
use mre_widgets::view::IconKind;
use mre_widgets::{Role, Theme, View};
use std::collections::HashMap;

/// Event handlers collected during conversion, keyed by the node's key.
/// A property driven directly by an engine spring, bypassing the view function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Node key the property applies to.
    pub key: String,
    /// Which property.
    pub property: AnimProp,
    /// Spring id, as returned on the handle.
    pub spring: u64,
}

fn parse_anim_prop(name: &str) -> Option<AnimProp> {
    match name {
        "opacity" => Some(AnimProp::Opacity),
        "x" | "translate_x" => Some(AnimProp::X),
        "y" | "translate_y" => Some(AnimProp::Y),
        _ => None,
    }
}

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

/// A Lua value read as a string, accepting numbers as text.
fn string_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_string_lossy()),
        Value::Integer(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

/// A Lua value read as a number.
///
/// Lua 5.4 keeps integers and floats apart, so both are accepted here.
fn number_value(value: &Value) -> Option<f32> {
    match value {
        Value::Integer(value) => Some(*value as f32),
        Value::Number(value) => Some(*value as f32),
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

fn display_of(text: &str) -> Option<Display> {
    match text {
        "none" | "hidden" => Some(Display::None),
        "flex" | "row" | "column" => Some(Display::Flex),
        "grid" => Some(Display::Grid),
        "stack" | "layer" => Some(Display::Stack),
        _ => None,
    }
}

fn direction_of(text: &str) -> Option<Direction> {
    match text {
        "row" => Some(Direction::Row),
        "column" => Some(Direction::Column),
        "row-reverse" => Some(Direction::RowReverse),
        "column-reverse" => Some(Direction::ColumnReverse),
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

/// Everything the converter reads from one vnode, gathered in a single pass.
///
/// Looking each property up separately cost about 15µs per node, which was the
/// dominant cost of a rebuild. Walking the table once and matching the keys
/// costs about 2µs, because a property that was never set is never looked up.
#[derive(Default)]
struct Fields {
    kind: Option<String>,
    text: Option<String>,
    size: Option<f32>,
    weight: Option<f32>,
    italic: Option<bool>,
    line_height: Option<f32>,
    letter_spacing: Option<f32>,
    family: Option<String>,
    text_align: Option<TextAlign>,
    nowrap: Option<bool>,
    value: Option<String>,
    placeholder: Option<String>,
    password: bool,
    icon: Option<String>,
    count: Option<f32>,
    row_height: Option<f32>,
    offset: Option<f32>,
    padding: Option<Edges>,
    margin: Option<Edges>,
    gap: Option<f32>,
    width: Option<Dim>,
    height: Option<Dim>,
    min_width: Option<Dim>,
    max_width: Option<Dim>,
    min_height: Option<Dim>,
    max_height: Option<Dim>,
    grow: Option<f32>,
    shrink: Option<f32>,
    basis: Option<Dim>,
    aspect_ratio: Option<f32>,
    display: Option<Display>,
    direction: Option<Direction>,
    rows: Vec<Track>,
    align: Option<Align>,
    justify: Option<Justify>,
    absolute: bool,
    overlay: bool,
    top: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    left: Option<f32>,
    background: Option<Rgba8>,
    color: Option<Rgba8>,
    radius: Option<f32>,
    opacity: Option<f32>,
    interactive: bool,
    focusable: bool,
    selectable: bool,
    tint: Option<bool>,
    clip: bool,
    border: Option<(Rgba8, f32)>,
    role: Option<Role>,
    label: Option<String>,
    key: Option<String>,
    on_click: Option<Function>,
    on_scroll: Option<Function>,
    on_drag: Option<Function>,
    on_change: Option<Function>,
    on_submit: Option<Function>,
    animate: Option<Table>,
    columns: Vec<Track>,
    render: Option<Function>,
    children: Option<Table>,
    diagnostics: Vec<String>,
}

/// Reads every property of one vnode in a single walk of the table.
fn fields_from(table: &Table, theme: &Theme) -> mlua::Result<Fields> {
    let mut fields = Fields::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        // The array part of a vnode carries the children; it is read once, from
        // the normalised `children` table, so numeric keys are skipped here.
        let Value::String(key) = key else {
            continue;
        };
        let key = key.to_str()?;
        match &*key {
            "__mre" => fields.kind = string_value(&value),
            "text" => fields.text = string_value(&value),
            "children" => fields.children = value.as_table().cloned(),
            "size" => fields.size = number_value(&value),
            "weight" => fields.weight = number_value(&value),
            "italic" => fields.italic = value.as_boolean(),
            "line_height" => fields.line_height = number_value(&value),
            "letter_spacing" => fields.letter_spacing = number_value(&value),
            "family" => fields.family = string_value(&value),
            "text_align" => {
                fields.text_align = string_value(&value).and_then(|value| text_align_of(&value));
            }
            "nowrap" => fields.nowrap = value.as_boolean(),
            "value" => fields.value = string_value(&value),
            "placeholder" => fields.placeholder = string_value(&value),
            "password" => fields.password = value.as_boolean().unwrap_or(false),
            "kind" => fields.icon = string_value(&value),
            "count" => fields.count = number_value(&value),
            "row_height" => fields.row_height = number_value(&value),
            "offset" => fields.offset = number_value(&value),
            "padding" => fields.padding = edges(&value),
            "margin" => fields.margin = edges(&value),
            "gap" => fields.gap = number_value(&value),
            "width" => fields.width = dimension(&value),
            "height" => fields.height = dimension(&value),
            "min_width" => fields.min_width = dimension(&value),
            "max_width" => fields.max_width = dimension(&value),
            "min_height" => fields.min_height = dimension(&value),
            "max_height" => fields.max_height = dimension(&value),
            "grow" => fields.grow = number_value(&value),
            "shrink" => fields.shrink = number_value(&value),
            "basis" => fields.basis = dimension(&value),
            "aspect_ratio" => fields.aspect_ratio = number_value(&value),
            "display" => fields.display = string_value(&value).and_then(|value| display_of(&value)),
            "direction" => {
                fields.direction = string_value(&value).and_then(|value| direction_of(&value))
            }
            "rows" => {
                if let Some(rows) = value.as_table() {
                    fields.rows = rows
                        .sequence_values::<Value>()
                        .filter_map(Result::ok)
                        .filter_map(|value| track(&value))
                        .collect();
                }
            }
            "align" => fields.align = string_value(&value).and_then(|value| align_of(&value)),
            "justify" => fields.justify = string_value(&value).and_then(|value| justify_of(&value)),
            "absolute" => fields.absolute = value.as_boolean().unwrap_or(false),
            "overlay" => fields.overlay = value.as_boolean().unwrap_or(false),
            "position" => {
                if string_value(&value).as_deref() == Some("absolute") {
                    fields.absolute = true;
                }
            }
            "inset" => {
                if let Some(inset) = value.as_table() {
                    if fields.top.is_none() {
                        fields.top = inset
                            .get::<Option<f64>>("top")
                            .ok()
                            .flatten()
                            .map(|v| v as f32);
                    }
                    if fields.right.is_none() {
                        fields.right = inset
                            .get::<Option<f64>>("right")
                            .ok()
                            .flatten()
                            .map(|v| v as f32);
                    }
                    if fields.bottom.is_none() {
                        fields.bottom = inset
                            .get::<Option<f64>>("bottom")
                            .ok()
                            .flatten()
                            .map(|v| v as f32);
                    }
                    if fields.left.is_none() {
                        fields.left = inset
                            .get::<Option<f64>>("left")
                            .ok()
                            .flatten()
                            .map(|v| v as f32);
                    }
                }
            }
            "top" => fields.top = number_value(&value),
            "right" => fields.right = number_value(&value),
            "bottom" => fields.bottom = number_value(&value),
            "left" => fields.left = number_value(&value),
            "background" => fields.background = colour_value(&value, theme),
            "color" => fields.color = colour_value(&value, theme),
            "radius" => fields.radius = number_value(&value),
            "opacity" => fields.opacity = number_value(&value),
            "interactive" => fields.interactive = value.as_boolean().unwrap_or(false),
            "focusable" => fields.focusable = value.as_boolean().unwrap_or(false),
            "selectable" => fields.selectable = value.as_boolean().unwrap_or(false),
            "tint" => fields.tint = value.as_boolean(),
            "clip" => fields.clip |= value.as_boolean().unwrap_or(false),
            "overflow" => fields.clip |= string_value(&value).as_deref() == Some("hidden"),
            "border" => fields.border = border_value(&value, theme),
            "role" => fields.role = string_value(&value).and_then(|name| Role::parse(&name)),
            "label" => fields.label = string_value(&value),
            "key" => fields.key = string_value(&value),
            "on_click" => fields.on_click = value.as_function().cloned(),
            "on_scroll" => fields.on_scroll = value.as_function().cloned(),
            "on_drag" => fields.on_drag = value.as_function().cloned(),
            "on_change" => fields.on_change = value.as_function().cloned(),
            "on_submit" => fields.on_submit = value.as_function().cloned(),
            "animate" => fields.animate = value.as_table().cloned(),
            "columns" => {
                if let Some(columns) = value.as_table() {
                    fields.columns = columns
                        .sequence_values::<Value>()
                        .filter_map(Result::ok)
                        .filter_map(|value| track(&value))
                        .collect();
                }
            }
            "render" => fields.render = value.as_function().cloned(),
            _ => {
                // A typo must be visible. The engine keeps the last good tree,
                // but the app can read these through mre.diagnostics().
                let name = key.to_string_lossy().to_string();
                if !name.starts_with("__") && name != "children" {
                    fields.diagnostics.push(format!(
                        "unknown property '{name}' on {}",
                        fields.kind.as_deref().unwrap_or("node")
                    ));
                }
            }
        }
    }
    Ok(fields)
}

/// A border, given either as a table or as a plain colour.
fn border_value(value: &Value, theme: &Theme) -> Option<(Rgba8, f32)> {
    match value {
        Value::Table(border) => {
            let colour = border
                .get::<Value>("color")
                .ok()
                .and_then(|value| colour_value(&value, theme))
                .unwrap_or(theme.border);
            let width = border
                .get::<Option<f64>>("width")
                .ok()
                .flatten()
                .map_or(1.0, |value| value as f32);
            Some((colour, width))
        }
        _ => colour_value(value, theme).map(|colour| (colour, 1.0)),
    }
}

/// The text style described by a node's fields.
fn text_style_from(fields: &Fields, theme: &Theme) -> TextStyle {
    let mut style = TextStyle::new(fields.size.unwrap_or(theme.font_size));
    if let Some(weight) = fields.weight {
        style.weight = weight.clamp(100.0, 900.0) as u16;
    }
    if let Some(italic) = fields.italic {
        style.italic = italic;
    }
    if let Some(line_height) = fields.line_height {
        style.line_height = Some(line_height);
    }
    if let Some(spacing) = fields.letter_spacing {
        style.letter_spacing = spacing;
    }
    if let Some(family) = fields.family.as_ref() {
        style.family = Some(family.clone());
    }
    if let Some(align) = fields.text_align {
        style.align = align;
    }
    if fields.nowrap.unwrap_or(false) {
        style.wrap = Wrap::None;
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
    bindings: &mut Vec<Binding>,
    counter: &mut u32,
    diagnostics: &mut Vec<String>,
) -> mlua::Result<View> {
    let fields = fields_from(table, theme)?;
    diagnostics.extend(fields.diagnostics.iter().cloned());
    let kind = fields.kind.clone().unwrap_or_else(|| "column".to_string());

    let mut view = match kind.as_str() {
        "text" => {
            let text = fields.text.clone().unwrap_or_default();
            let style = text_style_from(&fields, theme);
            let colour = fields.color.unwrap_or(theme.text);
            View::text(text, style).color(colour)
        }
        "button" => {
            let label = fields.text.clone().or_else(|| {
                // `mre.button { "Refresh" }` puts the label in the child array.
                fields
                    .children
                    .as_ref()
                    .and_then(|children| children.get::<Option<String>>(1).ok().flatten())
            });
            View::button(label.unwrap_or_default())
        }
        "spacer" => View::spacer(fields.size.unwrap_or(0.0)),
        "text_input" => {
            let value = fields.value.clone().unwrap_or_default();
            let placeholder = fields.placeholder.clone().unwrap_or_default();
            let size = fields.size.unwrap_or(14.0);
            let mut view = View::text_input(value, placeholder, size);
            // A field's text can be selected; a label's cannot.
            view.props.selectable = true;
            if fields.password {
                if let mre_widgets::ViewKind::TextInput(input) = &mut view.kind {
                    input.password = true;
                }
            }
            view
        }
        "icon" => {
            let kind = fields
                .icon
                .as_deref()
                .and_then(IconKind::parse)
                .unwrap_or(IconKind::Check);
            let size = fields.size.unwrap_or(16.0);
            View::icon(kind, size)
        }
        "list" => {
            let count = fields.count.unwrap_or(0.0).max(0.0) as usize;
            let row_height = fields.row_height.unwrap_or(24.0).max(1.0);
            let offset = fields.offset.unwrap_or(0.0).max(0.0);
            let viewport = match fields.height {
                Some(Dim::Px(value)) => value.max(row_height),
                _ => 400.0_f32.max(row_height),
            };

            let total = count as f32 * row_height;
            let mut view = View::stack().height(Dim::Px(total)).clip(true);

            let first = (offset / row_height).floor().max(0.0) as usize;
            let visible = (viewport / row_height).ceil() as usize + 3;
            let start = first.saturating_sub(1).min(count);
            let end = (first + visible).min(count);

            if let Some(render) = fields.render.as_ref() {
                for index in start..end {
                    let Ok(Some(child)) = render.call::<Option<Table>>(index as i64 + 1) else {
                        continue;
                    };
                    let mut child =
                        convert(&child, theme, handlers, bindings, counter, diagnostics)?;
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
        "grid" => View::grid(fields.columns.clone()),
        "stack" | "layer" => View::stack(),
        "row" => View::row(),
        "scroll" => View::column().clip(true),
        // `scroll`, `image` and `canvas` are containers until their dedicated
        // implementations land; they must still lay out their children.
        _ => View::column(),
    };

    // Layout properties.
    if let Some(padding) = fields.padding {
        view.style.padding = padding;
    }
    if let Some(margin) = fields.margin {
        view.style.margin = margin;
    }
    if let Some(gap) = fields.gap {
        view.style.gap = gap.max(0.0);
    }
    if let Some(width) = fields.width {
        view.style.width = width;
    }
    if kind != "list" {
        if let Some(height) = fields.height {
            view.style.height = height;
        }
    }
    if let Some(width) = fields.min_width {
        view.style.min_width = width;
    }
    if let Some(width) = fields.max_width {
        view.style.max_width = width;
    }
    if let Some(height) = fields.min_height {
        view.style.min_height = height;
    }
    if let Some(height) = fields.max_height {
        view.style.max_height = height;
    }
    if let Some(display) = fields.display {
        view.style.display = display;
    }
    if let Some(direction) = fields.direction {
        view.style.direction = direction;
    }
    if let Some(grow) = fields.grow {
        view.style.flex_grow = grow.max(0.0);
    }
    if let Some(shrink) = fields.shrink {
        view.style.flex_shrink = shrink.max(0.0);
    }
    if let Some(basis) = fields.basis {
        view.style.flex_basis = basis;
    }
    if let Some(ratio) = fields.aspect_ratio {
        if ratio.is_finite() && ratio > 0.0 {
            view.style.aspect_ratio = Some(ratio);
        }
    }
    if !fields.rows.is_empty() {
        view.style.rows = fields.rows;
    }
    if let Some(align) = fields.align {
        view.style.align_items = align;
    }
    if let Some(justify) = fields.justify {
        view.style.justify_content = justify;
    }
    if fields.absolute || fields.overlay {
        view.style.position = Position::Absolute;
        view.style.inset = [fields.top, fields.right, fields.bottom, fields.left];
        view.style.overlay = fields.overlay;
    }

    // Paint properties.
    if let Some(background) = fields.background {
        view.props.background = Some(background);
    }
    if let Some(radius) = fields.radius {
        view.props.radius = radius.max(0.0);
    }
    if let Some(opacity) = fields.opacity {
        view.props.opacity = opacity.clamp(0.0, 1.0);
    }
    if fields.interactive {
        view.props.interactive = true;
    }
    if fields.focusable {
        view.props.focusable = true;
    }
    if fields.selectable {
        view.props.selectable = true;
    }
    if let Some(tint) = fields.tint {
        view.props.tint = tint;
    }
    if fields.clip {
        view.props.clip = true;
    }
    if kind != "text" {
        if let Some(colour) = fields.color {
            view.props.color = Some(colour);
        }
    }
    if let Some(border) = fields.border {
        view.props.border = Some(border);
    }

    // Children.
    if let Some(children) = fields.children.as_ref() {
        for child in children.sequence_values::<Table>().filter_map(Result::ok) {
            view.children.push(convert(
                &child,
                theme,
                handlers,
                bindings,
                counter,
                diagnostics,
            )?);
        }
    }

    // Semantics.
    if let Some(role) = fields.role {
        view.props.role = Some(role);
    }
    if let Some(label) = fields.label {
        view.props.label = Some(label);
    }

    // Identity.
    if let Some(key) = fields.key {
        view.key = Some(key);
    }

    // Events.
    if let Some(handler) = fields.on_click {
        let key = ensure_key(&mut view.key, counter);
        view.props.interactive = true;
        handlers.click.insert(key, handler);
    }
    if let Some(handler) = fields.on_scroll {
        let key = ensure_key(&mut view.key, counter);
        handlers.scroll.insert(key, handler);
    }
    if let Some(handler) = fields.on_drag {
        let key = ensure_key(&mut view.key, counter);
        view.props.interactive = true;
        handlers.drag.insert(key, handler);
    }
    if let Some(handler) = fields.on_change {
        let key = ensure_key(&mut view.key, counter);
        handlers.change.insert(key, handler);
    }
    if let Some(handler) = fields.on_submit {
        let key = ensure_key(&mut view.key, counter);
        handlers.submit.insert(key, handler);
    }

    // Animated properties. The handle is a table carrying a spring id; no Lua
    // runs per frame, because the runtime reads the spring directly.
    if let Some(animate) = fields.animate.as_ref() {
        for pair in animate.pairs::<String, Table>() {
            let (name, handle) = pair?;
            let Some(property) = parse_anim_prop(&name) else {
                continue;
            };
            let Some(spring) = handle.get::<Option<u64>>("id")? else {
                continue;
            };
            let key = ensure_key(&mut view.key, counter);
            bindings.push(Binding {
                key,
                property,
                spring,
            });
        }
    }

    Ok(view)
}

/// A view plus the handlers it declared.
pub struct ConvertedView {
    /// The root view.
    pub view: View,
    /// Handlers keyed by node key.
    pub handlers: Handlers,
    /// Properties driven directly by springs.
    pub bindings: Vec<Binding>,
    /// Non-fatal problems found while converting the tree, for `mre.diagnostics()`.
    pub diagnostics: Vec<String>,
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
    let mut bindings = Vec::new();
    let mut counter = 0;
    let mut diagnostics = Vec::new();
    let view = convert(
        table,
        theme,
        &mut handlers,
        &mut bindings,
        &mut counter,
        &mut diagnostics,
    )?;
    Ok(ConvertedView {
        view,
        handlers,
        bindings,
        diagnostics,
    })
}

/// Counts nodes in a converted view, for diagnostics.
#[must_use]
pub fn count(view: &View) -> usize {
    1 + view.children.iter().map(count).sum::<usize>()
}
