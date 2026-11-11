//! Parsing of engineering values such as `10kΩ`, `100nF`, `62.5mW`, `±1%`.

/// Parse the leading engineering value of an attribute string into base
/// units. Anything after `@` (test conditions) is ignored.
///
/// ```
/// use jlcpcb_mcp::values::parse_si;
/// assert_eq!(parse_si("10kΩ"), Some(10_000.0));
/// assert_eq!(parse_si("1.1V@(800mA)"), Some(1.1));
/// assert!((parse_si("100nF").unwrap() - 1e-7).abs() < 1e-18);
/// ```
pub fn parse_si(raw: &str) -> Option<f64> {
    let s = raw.split('@').next()?.trim();
    let s = s.trim_start_matches(['±', '+']);
    let bytes: Vec<char> = s.chars().collect();
    let mut end = 0;
    let mut seen_digit = false;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if !(c == '.' || (c == '-' && end == 0)) {
            break;
        }
        end += 1;
    }
    if !seen_digit {
        return None;
    }
    let number: f64 = bytes[..end].iter().collect::<String>().parse().ok()?;
    let rest: String = bytes[end..].iter().collect();
    let rest = rest.trim_start();
    let mut chars = rest.chars();
    let multiplier = match chars.next() {
        Some('p') => 1e-12,
        Some('n') => 1e-9,
        Some('u') | Some('µ') | Some('μ') => 1e-6,
        Some('m') => 1e-3,
        Some('k') | Some('K') => 1e3,
        Some('M') => 1e6,
        Some('G') => 1e9,
        _ => 1.0,
    };
    Some(number * multiplier)
}

/// Parse a tolerance such as `±1%` or `±0.1%` into a percentage.
pub fn parse_tolerance_pct(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if !s.ends_with('%') {
        return None;
    }
    s.trim_start_matches('±')
        .trim_end_matches('%')
        .trim()
        .parse()
        .ok()
}

/// Rank a ceramic capacitor dielectric: higher is more stable.
pub fn dielectric_rank(raw: &str) -> Option<u8> {
    let s = raw.trim().to_ascii_uppercase();
    match s.as_str() {
        "C0G" | "NP0" | "COG" | "C0G(NP0)" | "NP0(C0G)" => Some(4),
        "X8R" | "X7R" | "X7S" | "X7T" => Some(3),
        "X6S" | "X5R" => Some(2),
        "Y5V" | "Z5U" | "Y5U" => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= b.abs() * 1e-9 + 1e-18
    }

    #[test]
    fn parses_common_values() {
        assert!(close(parse_si("10kΩ").unwrap(), 10_000.0));
        assert!(close(parse_si("4.7uF").unwrap(), 4.7e-6));
        assert!(close(parse_si("100nF").unwrap(), 1e-7));
        assert!(close(parse_si("62.5mW").unwrap(), 0.0625));
        assert!(close(parse_si("50V").unwrap(), 50.0));
        assert!(close(parse_si("1MΩ").unwrap(), 1e6));
        assert!(close(parse_si("450mΩ").unwrap(), 0.45));
        assert!(close(parse_si("3.3V").unwrap(), 3.3));
        assert!(close(parse_si("1.1V@(800mA)").unwrap(), 1.1));
        assert!(close(parse_si("8MHz").unwrap(), 8e6));
        assert!(close(parse_si("0Ω").unwrap(), 0.0));
    }

    #[test]
    fn rejects_non_values() {
        assert_eq!(parse_si("-"), None);
        assert_eq!(parse_si("X7R"), None);
        assert_eq!(parse_si(""), None);
    }

    #[test]
    fn tolerance_and_dielectric() {
        assert_eq!(parse_tolerance_pct("±1%"), Some(1.0));
        assert_eq!(parse_tolerance_pct("±0.5%"), Some(0.5));
        assert_eq!(parse_tolerance_pct("5pF"), None);
        assert!(dielectric_rank("C0G").unwrap() > dielectric_rank("X7R").unwrap());
        assert!(dielectric_rank("x5r").unwrap() > dielectric_rank("Y5V").unwrap());
        assert_eq!(dielectric_rank("weird"), None);
    }
}
