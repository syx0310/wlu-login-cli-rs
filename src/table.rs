use serde::Serialize;
use serde_json::Value;

pub fn print<T: Serialize>(value: &T) {
    let Ok(Value::Object(map)) = serde_json::to_value(value) else {
        return;
    };

    let rows: Vec<(String, String)> = map
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| {
            let display = match value {
                Value::String(value) => value,
                other => other.to_string(),
            };
            (name, display)
        })
        .collect();

    if rows.is_empty() {
        return;
    }

    let name_width = rows
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or("Name".len())
        .max("Name".len());
    let value_width = rows
        .iter()
        .map(|(_, value)| value.len())
        .max()
        .unwrap_or("Value".len())
        .max("Value".len());

    print_border(name_width, value_width);
    println!(
        "| {name:<name_width$} | {value:<value_width$} |",
        name = "Name",
        value = "Value"
    );
    print_border(name_width, value_width);
    for (name, value) in rows {
        println!("| {name:<name_width$} | {value:<value_width$} |");
    }
    print_border(name_width, value_width);
}

fn print_border(name_width: usize, value_width: usize) {
    println!(
        "+-{:-<name_width$}-+-{:-<value_width$}-+",
        "",
        "",
        name_width = name_width,
        value_width = value_width
    );
}
