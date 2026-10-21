//! RFC 8785 JSON Canonicalization Scheme (JCS), the canonical form that
//! did:webvh hashes and eddsa-jcs-2022 signs.

use std::fmt::Write as _;

use serde_json::{Number, Value};

/// Largest magnitude an integer can have and still print as itself under
/// ECMAScript `Number` serialization (RFC 8785 §3.2.2.3, Appendix B note 1).
const MAX_SAFE_MAGNITUDE: u64 = 1 << 53;

/// The JCS form of `value`, or `None` when a number is not finite (RFC
/// 8785 §3.2.2.3: NaN and Infinity "MUST cause a compliant JCS
/// implementation to terminate with an appropriate error").
pub(crate) fn canonicalize(value: &Value) -> Option<String> {
    let mut out = String::new();
    write_value(value, &mut out)?;
    Some(out)
}

fn write_value(value: &Value, out: &mut String) -> Option<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(n, out)?,
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            // RFC 8785 §3.2.3: sort by UTF-16 code units.
            let mut members: Vec<_> = map.iter().collect();
            members.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, (name, member)) in members.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(name, out);
                out.push(':');
                write_value(member, out)?;
            }
            out.push('}');
        }
    }
    Some(())
}

/// RFC 8785 §3.2.2.2 string serialization.
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_number(n: &Number, out: &mut String) -> Option<()> {
    if let Some(i) = n
        .as_i64()
        .filter(|i| i.unsigned_abs() <= MAX_SAFE_MAGNITUDE)
    {
        let _ = write!(out, "{i}");
        return Some(());
    }
    if let Some(u) = n.as_u64().filter(|&u| u <= MAX_SAFE_MAGNITUDE) {
        let _ = write!(out, "{u}");
        return Some(());
    }
    write_f64(n.as_f64().filter(|x| x.is_finite())?, out);
    Some(())
}

/// ECMAScript `Number::toString` (ECMA-262 §7.1.12.1, with Note 2).
///
/// Rust's `{:e}` gives the fewest digits that round-trip, but not always
/// the closest of them to `x` (RFC 8785 Appendix B note 4:
/// `1424953923781206.25` must print as `...206.2`). Formatting to that
/// many digits is correctly rounded, half to even, so it is used when it
/// also round-trips.
fn write_f64(x: f64, out: &mut String) {
    if x == 0.0 {
        out.push('0');
        return;
    }
    if x < 0.0 {
        out.push('-');
    }
    let shortest = format!("{:e}", x.abs());
    let precision = shortest
        .split_once('e')
        .map_or(0, |(m, _)| m.len().saturating_sub(2));
    let closest = format!("{:.precision$e}", x.abs());
    let sci = if closest.parse::<f64>() == Ok(x.abs()) {
        closest
    } else {
        shortest
    };
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    // x = 0.digits × 10^n
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        let _ = write!(out, "{int}.{frac}");
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            let _ = write!(out, ".{rest}");
        }
        let e = n - 1;
        let _ = write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn number(bits: u64) -> String {
        let mut out = String::new();
        write_f64(f64::from_bits(bits), &mut out);
        out
    }

    /// RFC 8785 Appendix B, Table 1, every finite row.
    #[test]
    fn numbers_match_rfc8785_appendix_b() {
        let table = [
            (0x0000000000000000, "0"),
            (0x8000000000000000, "0"),
            (0x0000000000000001, "5e-324"),
            (0x8000000000000001, "-5e-324"),
            (0x7fefffffffffffff, "1.7976931348623157e+308"),
            (0xffefffffffffffff, "-1.7976931348623157e+308"),
            (0x4340000000000000, "9007199254740992"),
            (0xc340000000000000, "-9007199254740992"),
            (0x4430000000000000, "295147905179352830000"),
            (0x44b52d02c7e14af5, "9.999999999999997e+22"),
            (0x44b52d02c7e14af6, "1e+23"),
            (0x44b52d02c7e14af7, "1.0000000000000001e+23"),
            (0x444b1ae4d6e2ef4e, "999999999999999700000"),
            (0x444b1ae4d6e2ef4f, "999999999999999900000"),
            (0x444b1ae4d6e2ef50, "1e+21"),
            (0x3eb0c6f7a0b5ed8c, "9.999999999999997e-7"),
            (0x3eb0c6f7a0b5ed8d, "0.000001"),
            (0x41b3de4355555553, "333333333.3333332"),
            (0x41b3de4355555554, "333333333.33333325"),
            (0x41b3de4355555555, "333333333.3333333"),
            (0x41b3de4355555556, "333333333.3333334"),
            (0x41b3de4355555557, "333333333.33333343"),
            (0xbecbf647612f3696, "-0.0000033333333333333333"),
            (0x43143ff3c1cb0959, "1424953923781206.2"),
        ];
        for (bits, expected) in table {
            assert_eq!(number(bits), expected, "{bits:016x}");
        }
    }

    /// RFC 8785 §3.2.2 input and its §3.2.3 canonical form.
    #[test]
    fn rfc8785_sample_canonicalizes() {
        let input: Value = serde_json::from_str(
            r#"{
                "numbers": [333333333.33333329, 1E30, 4.50,
                            2e-3, 0.000000000000000000000000001],
                "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
                "literals": [null, true, false]
            }"#,
        )
        .unwrap();
        assert_eq!(
            canonicalize(&input).unwrap(),
            "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,\
             1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}"
        );
    }

    /// RFC 8785 §3.2.3 sorting test data and its expected value order.
    #[test]
    fn properties_sort_by_utf16_code_units() {
        let input: Value = serde_json::from_str(
            r#"{
                "\u20ac": "Euro Sign",
                "\r": "Carriage Return",
                "\ufb33": "Hebrew Letter Dalet With Dagesh",
                "1": "One",
                "\ud83d\ude00": "Emoji: Grinning Face",
                "\u0080": "Control",
                "\u00f6": "Latin Small Letter O With Diaeresis"
            }"#,
        )
        .unwrap();
        let text = canonicalize(&input).unwrap();
        let canonical: Value = serde_json::from_str(&text).unwrap();
        let order: Vec<usize> = [
            "Carriage Return",
            "One",
            "Control",
            "Latin Small Letter O With Diaeresis",
            "Euro Sign",
            "Emoji: Grinning Face",
            "Hebrew Letter Dalet With Dagesh",
        ]
        .iter()
        .map(|v| text.find(v).unwrap())
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert_eq!(canonical, input);
    }

    #[test]
    fn control_characters_use_lowercase_hex_escapes() {
        // RFC 8785 §3.2.2.2: U+0000..U+001F other than \b \t \n \f \r as
        // lowercase \uhhhh; everything else, U+007F included, as is.
        assert_eq!(
            canonicalize(&json!("\u{1}\u{8}\u{1f}\u{7f}")).unwrap(),
            "\"\\u0001\\b\\u001f\u{7f}\""
        );
    }
}
