use mlua::Lua;
use std::time::Instant;

fn main() -> mlua::Result<()> {
    let lua = Lua::new();
    let table = lua.create_table()?;
    let keys = [
        "width",
        "height",
        "padding",
        "margin",
        "gap",
        "align",
        "justify",
        "grow",
        "shrink",
        "basis",
        "background",
        "color",
        "radius",
        "border",
        "opacity",
        "key",
        "size",
        "weight",
        "italic",
        "family",
        "line_height",
        "letter_spacing",
        "text_align",
        "wrap",
        "clip",
        "interactive",
        "focusable",
        "role",
        "label",
        "min_width",
        "max_width",
        "min_height",
        "max_height",
        "absolute",
        "top",
        "right",
        "bottom",
        "left",
        "display",
        "direction",
    ];
    for (index, key) in keys.iter().enumerate() {
        table.set(*key, index as f64)?;
    }

    let n = 20_000;
    let start = Instant::now();
    let mut sum = 0.0;
    for _ in 0..n {
        for key in keys {
            let value: Option<f64> = table.get(key)?;
            sum += value.unwrap_or(0.0);
        }
    }
    let each = start.elapsed() / (n * keys.len() as u32);
    println!("get by &str: {each:?}/lookup (sum {sum})");

    let interned: Vec<mlua::LuaString> = keys
        .iter()
        .map(|key| lua.create_string(key))
        .collect::<mlua::Result<_>>()?;
    let start = Instant::now();
    for _ in 0..n {
        for key in &interned {
            let value: Option<f64> = table.raw_get(key.clone())?;
            sum += value.unwrap_or(0.0);
        }
    }
    let each = start.elapsed() / (n * keys.len() as u32);
    println!("raw_get interned: {each:?}/lookup (sum {sum})");

    let start = Instant::now();
    for _ in 0..n {
        for pair in table.pairs::<String, f64>() {
            let (_key, value) = pair?;
            sum += value;
        }
    }
    let each = start.elapsed() / (n * keys.len() as u32);
    println!("pairs: {each:?}/entry (sum {sum})");
    // Walk cost for a node with a realistic mix of property kinds.
    let node = lua.create_table()?;
    node.set("__mre", "row")?;
    node.set("height", 28.0)?;
    node.set("padding", lua.create_table()?)?;
    node.set("gap", 12.0)?;
    node.set("background", "#F7F8FA")?;
    node.set("text", "Instrument 00001")?;
    node.set("size", 13.0)?;
    node.set("width", 240.0)?;
    node.set("color", "#111827")?;
    node.set("key", "row-1")?;
    node.set("on_click", lua.create_function(|_, ()| Ok(()))?)?;
    node.set("children", lua.create_table()?)?;

    let n = 50_000;
    let start = Instant::now();
    let mut seen = 0_u32;
    for _ in 0..n {
        for pair in node.pairs::<mlua::Value, mlua::Value>() {
            let (key, _value) = pair?;
            if let mlua::Value::String(text) = key {
                let _ = text.to_str()?;
                seen += 1;
            }
        }
    }
    let each = start.elapsed() / n;
    println!("pairs walk over 11 keys: {each:?}/node (seen {seen})");

    let start = Instant::now();
    for _ in 0..n {
        let mut count = 0_u32;
        node.for_each(|key: mlua::Value, _value: mlua::Value| {
            if let mlua::Value::String(text) = key {
                let _ = text.to_str()?;
                count += 1;
            }
            Ok(())
        })?;
        seen += count;
    }
    let each = start.elapsed() / n;
    println!("for_each over 11 keys: {each:?}/node (seen {seen})");

    let start = Instant::now();
    for _ in 0..n {
        let _ = node.raw_get::<mlua::Value>("__mre")?;
        let _ = node.raw_get::<mlua::Value>("height")?;
        let _ = node.raw_get::<mlua::Value>("padding")?;
    }
    println!("3 raw_get: {:?}/node", start.elapsed() / n);

    // Flat-array transfer: how fast can a Lua array cross into Rust?
    let array = lua.create_table()?;
    for index in 0..100 {
        array.set(index + 1, (index as f64) * 1.5)?;
    }
    let n = 20_000;
    let start = Instant::now();
    let mut total = 0.0;
    for _ in 0..n {
        for value in array.sequence_values::<f64>() {
            total += value?;
        }
    }
    println!(
        "sequence_values f64: {:?}/node (total {total})",
        start.elapsed() / n
    );

    let start = Instant::now();
    let mut total = 0.0;
    for _ in 0..n {
        for value in array.sequence_values::<mlua::Value>() {
            if let mlua::Value::Number(number) = value? {
                total += number;
            }
        }
    }
    println!(
        "sequence_values Value: {:?}/node (total {total})",
        start.elapsed() / n
    );

    Ok(())
}
