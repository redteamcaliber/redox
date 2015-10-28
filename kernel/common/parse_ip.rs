
/// Get the port from a string (ip)
pub fn parse_port(string: &str) -> &str {
    let mut a = 0;
    let mut b = 0;

    for (n, c) in string.chars().enumerate() {
        match c {
            '0' | '1' |
            '2' | '3' |
            '4' | '5' |
            '6' | '7' |
            '8' | '9' => b += 1,
            ':' => a = n + 1,
            _ => break,
        }
    }

    &string[a..b + 1]

}

/// Get the host from a string (ip)
pub fn parse_host(string: &str) -> &str {
    let mut b = 0;

    // TODO: Username/Password syntax
    for (n, c) in string.chars().enumerate() {
        match c {
            ':' | '/' => break,
            _ => b += 1,
        }
    }

    &string[..b + 1]

}
