//! Declarative views.
//!
//! A [`View`] is a plain data tree: no callbacks, no interior mutability, no
//! lifetime tied to the UI. Building it is pure, and turning it into geometry is
//! a single recursive pass. That split is what makes the Lua bridge easy: Lua
//! builds this tree, Rust lays it out.

use mre_core::Rgba8;
use mre_layout::{Align, Dim, Edges, Justify, Style, Track};
use mre_text::TextStyle;

/// What a node means to an assistive technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Role {
    /// A generic grouping container.
    #[default]
    Group,
    /// A run of static text.
    Text,
    /// A button.
    Button,
    /// An editable single-line field.
    TextField,
    /// A checkbox.
    CheckBox,
    /// A toggle switch.
    Switch,
    /// A slider.
    Slider,
    /// A progress indicator.
    ProgressBar,
    /// A scrollable region.
    ScrollView,
    /// A virtualised list.
    List,
    /// A strip of tabs.
    TabList,
    /// One tab in a tab strip.
    Tab,
    /// A modal dialog.
    Dialog,
    /// A short description of another control.
    Tooltip,
    /// A combo box.
    ComboBox,
    /// Decorative or informative image.
    Image,
}

impl Role {
    /// Parses a role name from a view.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "group" | "container" => Some(Self::Group),
            "text" | "label" => Some(Self::Text),
            "button" => Some(Self::Button),
            "textfield" | "text_field" | "textbox" => Some(Self::TextField),
            "checkbox" | "check_box" => Some(Self::CheckBox),
            "switch" | "toggle" => Some(Self::Switch),
            "slider" => Some(Self::Slider),
            "progress" | "progressbar" => Some(Self::ProgressBar),
            "scroll" | "scrollview" => Some(Self::ScrollView),
            "list" => Some(Self::List),
            "tablist" | "tabs" => Some(Self::TabList),
            "tab" => Some(Self::Tab),
            "dialog" | "modal" => Some(Self::Dialog),
            "tooltip" => Some(Self::Tooltip),
            "combobox" | "select" => Some(Self::ComboBox),
            "image" | "icon" => Some(Self::Image),
            _ => None,
        }
    }
}

/// A built-in vector icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    /// A tick.
    Check,
    /// A cross.
    Close,
    /// A downward chevron.
    ChevronDown,
    /// A rightward chevron.
    ChevronRight,
}

impl IconKind {
    /// Parses an icon name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "check" | "tick" => Some(Self::Check),
            "close" | "cross" | "x" => Some(Self::Close),
            "chevron-down" | "down" => Some(Self::ChevronDown),
            "chevron-right" | "right" => Some(Self::ChevronRight),
            _ => None,
        }
    }
}

/// What a view represents.
#[derive(Clone, Debug)]
pub enum ViewKind {
    /// A container with children.
    Container,
    /// A run of text. The layout node is a leaf sized by shaping.
    Text(String, TextStyle),
    /// A themed button with a centred label.
    Button(String),
    /// An empty node with a fixed height, used to push siblings apart.
    Spacer(f32),
    /// A vector icon drawn from line segments, so it stays crisp at any density.
    Icon(IconKind),
    /// A single-line editable text field.
    TextInput(TextInput),
}

/// What a text field holds and how it displays it.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInput {
    /// Current contents, as supplied by the view. The editor keeps its own copy
    /// while it has focus so typing cannot be clobbered by a stale render.
    pub value: String,
    /// Muted text shown when the field is empty.
    pub placeholder: String,
    /// Replaces each character with a bullet when true.
    pub password: bool,
    /// Font size, logical pixels.
    pub size: f32,
}

/// Visual properties that do not affect layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Props {
    /// Fill colour.
    pub background: Option<Rgba8>,
    /// Outline colour and width.
    pub border: Option<(Rgba8, f32)>,
    /// Corner radius, clamped to half the smaller side.
    pub radius: f32,
    /// Text colour for text nodes, or the label colour for buttons.
    pub color: Option<Rgba8>,
    /// Opacity multiplier, `0.0..=1.0`.
    pub opacity: f32,
    /// Whether children are clipped to this node's bounds. Scroll containers and
    /// virtualised lists set this.
    pub clip: bool,
    /// Whether this node reacts to hover and press. Buttons and any node with a
    /// click handler set this.
    pub interactive: bool,
    /// Whether hover and press change this node's background. Large surfaces
    /// such as a dialog scrim turn this off.
    pub tint: bool,
    /// Whether clicking this node gives it keyboard focus. Text fields set this.
    pub focusable: bool,
    /// Semantic role override, for assistive technology.
    pub role: Option<Role>,
    /// Accessible label override. Text-bearing nodes derive one automatically.
    pub label: Option<String>,
}

impl Default for Props {
    fn default() -> Self {
        Self {
            background: None,
            border: None,
            radius: 0.0,
            color: None,
            opacity: 1.0,
            clip: false,
            interactive: false,
            tint: true,
            focusable: false,
            role: None,
            label: None,
        }
    }
}

/// One node of a declarative view tree.
#[derive(Clone, Debug)]
pub struct View {
    /// What this node is.
    pub kind: ViewKind,
    /// Layout properties.
    pub style: Style,
    /// Paint properties.
    pub props: Props,
    /// Children, in paint order.
    pub children: Vec<View>,
    /// Stable identity, for diffing and for addressing a node from Lua.
    pub key: Option<String>,
}

impl View {
    /// A vertical container.
    #[must_use]
    pub fn column() -> Self {
        Self::container(Style::column())
    }

    /// A horizontal container.
    #[must_use]
    pub fn row() -> Self {
        Self::container(Style::row())
    }

    /// A container whose children overlap.
    #[must_use]
    pub fn stack() -> Self {
        Self::container(Style::stack())
    }

    /// A grid with the given column tracks.
    #[must_use]
    pub fn grid(columns: Vec<Track>) -> Self {
        Self::container(Style::grid(columns))
    }

    /// A container around an existing layout style.
    #[must_use]
    pub fn container(style: Style) -> Self {
        Self {
            kind: ViewKind::Container,
            style,
            props: Props::default(),
            children: Vec::new(),
            key: None,
        }
    }

    /// A run of text.
    #[must_use]
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            kind: ViewKind::Text(text.into(), style),
            style: Style::default(),
            props: Props::default(),
            children: Vec::new(),
            key: None,
        }
    }

    /// A themed button.
    #[must_use]
    pub fn button(label: impl Into<String>) -> Self {
        Self {
            kind: ViewKind::Button(label.into()),
            style: Style::default(),
            props: Props::default(),
            children: Vec::new(),
            key: None,
        }
    }

    /// A single-line text field.
    #[must_use]
    pub fn text_input(value: impl Into<String>, placeholder: impl Into<String>, size: f32) -> Self {
        Self {
            kind: ViewKind::TextInput(TextInput {
                value: value.into(),
                placeholder: placeholder.into(),
                password: false,
                size,
            }),
            style: Style::default().height(Dim::Px(32.0)),
            props: Props {
                focusable: true,
                ..Props::default()
            },
            children: Vec::new(),
            key: None,
        }
    }

    /// A vector icon.
    #[must_use]
    pub fn icon(kind: IconKind, size: f32) -> Self {
        Self {
            kind: ViewKind::Icon(kind),
            style: Style::default().width(Dim::Px(size)).height(Dim::Px(size)),
            props: Props::default(),
            children: Vec::new(),
            key: None,
        }
    }

    /// Fixed vertical space.
    #[must_use]
    pub fn spacer(height: f32) -> Self {
        Self {
            kind: ViewKind::Spacer(height),
            style: Style::default().height(Dim::Px(height.max(0.0))),
            props: Props::default(),
            children: Vec::new(),
            key: None,
        }
    }

    /// Appends a child.
    #[must_use]
    pub fn child(mut self, child: View) -> Self {
        self.children.push(child);
        self
    }

    /// Appends several children.
    #[must_use]
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator<Item = View>,
    {
        self.children.extend(children);
        self
    }

    /// Sets uniform padding.
    #[must_use]
    pub fn padding(mut self, padding: f32) -> Self {
        self.style.padding = Edges::all(padding.max(0.0));
        self
    }

    /// Sets explicit padding.
    #[must_use]
    pub fn padding_edges(mut self, padding: Edges) -> Self {
        self.style.padding = padding;
        self
    }

    /// Sets the gap between children.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = gap.max(0.0);
        self
    }

    /// Sets the width.
    #[must_use]
    pub fn width(mut self, width: Dim) -> Self {
        self.style.width = width;
        self
    }

    /// Sets the height.
    #[must_use]
    pub fn height(mut self, height: Dim) -> Self {
        self.style.height = height;
        self
    }

    /// Sets cross-axis alignment of children.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.style.align_items = align;
        self
    }

    /// Sets main-axis distribution of children.
    #[must_use]
    pub fn justify(mut self, justify: Justify) -> Self {
        self.style.justify_content = justify;
        self
    }

    /// Makes this node absorb free space.
    #[must_use]
    pub fn grow(mut self, factor: f32) -> Self {
        self.style.flex_grow = factor.max(0.0);
        self
    }

    /// Sets the fill colour.
    #[must_use]
    pub fn background(mut self, color: Rgba8) -> Self {
        self.props.background = Some(color);
        self
    }

    /// Sets the corner radius.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.props.radius = radius.max(0.0);
        self
    }

    /// Sets an outline.
    #[must_use]
    pub fn border(mut self, color: Rgba8, width: f32) -> Self {
        self.props.border = Some((color, width));
        self
    }

    /// Sets the text or label colour.
    #[must_use]
    pub fn color(mut self, color: Rgba8) -> Self {
        self.props.color = Some(color);
        self
    }

    /// Marks the node as hoverable and pressable.
    #[must_use]
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.props.interactive = interactive;
        self
    }

    /// Clips children to this node's bounds.
    #[must_use]
    pub fn clip(mut self, clip: bool) -> Self {
        self.props.clip = clip;
        self
    }

    /// Sets opacity.
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.props.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Sets a stable key.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Number of layout nodes this view will produce, including this one.
    ///
    /// A button desugars into a container plus a label, so it counts as two.
    #[must_use]
    pub fn node_count(&self) -> usize {
        let own = match self.kind {
            ViewKind::Button(_) => 2,
            _ => 1,
        };
        own + self.children.iter().map(Self::node_count).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_compose() {
        let view = View::column()
            .padding(16.0)
            .gap(8.0)
            .child(View::text("hello", TextStyle::new(14.0)))
            .child(View::button("Save"));

        assert_eq!(view.children.len(), 2);
        assert_eq!(view.node_count(), 4);
        assert_eq!(view.style.padding.left, 16.0);
    }

    #[test]
    fn opacity_clamps() {
        assert_eq!(View::column().opacity(2.0).props.opacity, 1.0);
        assert_eq!(View::column().opacity(-1.0).props.opacity, 0.0);
    }
}
