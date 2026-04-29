use std::fs;

pub fn fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("Cannot read fixture {}: {}", name, e))
}
